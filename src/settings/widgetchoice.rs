//! Explicit theme-widget replacement choice, shared by package cards and legacy selectors.
use super::{
    model::{CtlVal, Model},
    ui::{Ctl, Item, Kind, Row},
};
use crate::theme::{
    engine::apply::{self, WidgetChoice},
    model::Selection,
};
use serde_json::Value;

pub struct Pending {
    pub selection: Selection,
    pub reset_colors: bool,
    pub widgets: Value,
    pub style: Option<crate::theme::style::Style>,
}

pub fn request(m: &mut Model, sel: &Selection, reset_colors: bool) -> bool {
    let src = super::themepane::source(m);
    m.widget_choice = None;
    if !reset_colors && src.find(&src.selection().name).map(|p| p.0) == src.find(&sel.name).map(|p| p.0) {
        return false;
    }
    match apply::widgets_need_confirmation(&src, &m.paths, &sel.name) {
        Ok(true) => {
            m.widget_choice = Some(Pending { selection: sel.clone(), reset_colors, style: None, widgets: super::io::json_load(&m.paths.shell_json())["widgets"].clone() });
            m.dirty = true;
            m.sel.reset_scroll = true;
            true
        }
        _ => false,
    }
}

pub fn items(m: &Model) -> Vec<Item> {
    let Some(p) = &m.widget_choice else { return Vec::new() };
    vec![
        Item::Note { text: format!("{} includes widgets. Replace your custom widget layout? Nothing changes until you choose.", p.selection.name), warn: false },
        Item::Card(vec![Row::new("Widget layout")
            .ctl(Ctl::new("wc:keep", Kind::Button { label: "Keep my layout".into(), danger: false }))
            .ctl(Ctl::new("wc:replace", Kind::Button { label: "Use theme widgets".into(), danger: false }))
            .ctl(Ctl::new("wc:cancel", Kind::Button { label: "Cancel".into(), danger: false }))]),
    ]
}

pub fn apply(m: &mut Model, id: &str, v: CtlVal) {
    if v != CtlVal::Click {
        return;
    }
    let Some(p) = m.widget_choice.take() else { return };
    if id == "wc:cancel" {
        m.dirty = true;
        return;
    }
    let choice = match id {
        "wc:keep" => WidgetChoice::Keep,
        "wc:replace" => WidgetChoice::Replace,
        _ => {
            m.widget_choice = Some(p);
            return;
        }
    };
    let current = super::io::json_load(&m.paths.shell_json())["widgets"].clone();
    if choice == WidgetChoice::Replace && current != p.widgets {
        m.widget_choice = Some(Pending { widgets: current, ..p });
        m.say("Your widget layout changed. Review the replacement choice again.", true);
        return;
    }
    let src = super::themepane::source(m);
    let plan =
        if p.reset_colors { apply::plan_with_widgets(&src, &m.paths, &p.selection.name, &p.selection, choice) } else { apply::plan_selection_with_widgets(&src, &m.paths, &p.selection, choice) };
    match plan.and_then(|plan| apply::execute(&src, &m.paths, plan)) {
        Ok(a) => {
            crate::theme::live::reload();
            m.on_files_changed();
            if let Some(st) = p.style {
                super::themepane::apply_style(m, st);
            }
            m.say(format!("Applied {} with {}", a.theme_id, if choice == WidgetChoice::Keep { "your widget layout" } else { "the theme's widgets" }), false);
        }
        Err(e) => m.say(format!("Not applied: {e}"), true),
    }
    m.dirty = true;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{io, model::tests_support::model_with};
    use serde_json::json;

    #[test]
    fn theme_cards_wait_for_choice_and_cancel_keep_replace_are_safe() {
        let (mut m, _) = model_with("[general]\n");
        let custom = json!({"enabled":true,"allow_exec":false,"items":[{"use":"builtin:clock-card","x":345}],"future":7});
        io::json_set(&m.paths.shell_json(), "widgets", Some(custom.clone())).unwrap();
        let before = std::fs::read(m.paths.shell_json()).unwrap();
        m.apply("l:tcard:apply:nothing-light", CtlVal::Click);
        assert!(m.widget_choice.is_some());
        assert_eq!(std::fs::read(m.paths.shell_json()).unwrap(), before);
        m.apply("wc:cancel", CtlVal::Click);
        assert!(m.widget_choice.is_none());
        assert_eq!(std::fs::read(m.paths.shell_json()).unwrap(), before);
        m.apply("l:tcard:apply:nothing-light", CtlVal::Click);
        m.apply("wc:keep", CtlVal::Click);
        assert_eq!(io::json_load(&m.paths.shell_json())["widgets"]["items"], custom["items"]);
        m.apply("l:tcard:apply:nothing-light", CtlVal::Click);
        let mut changed = custom.clone();
        changed["items"][0]["x"] = json!(456);
        io::json_set(&m.paths.shell_json(), "widgets", Some(changed.clone())).unwrap();
        m.apply("wc:replace", CtlVal::Click);
        assert!(m.widget_choice.is_some(), "a changed layout must be reviewed again");
        assert_eq!(io::json_load(&m.paths.shell_json())["widgets"], changed);
        m.apply("wc:replace", CtlVal::Click);
        assert!(m.widget_choice.is_none());
        let w = io::json_load(&m.paths.shell_json())["widgets"].clone();
        assert_ne!(w["items"], custom["items"]);
        assert!(!apply::custom_widgets(&w));
        assert_eq!(w["allow_exec"], false);
        assert_eq!(w["future"], 7);
        m.apply("l:tcard:apply:nord", CtlVal::Click);
        assert!(m.widget_choice.is_none());
        assert_eq!(io::json_load(&m.paths.shell_json())["widgets"]["enabled"], false);
    }
}
