//! Cached app indicators. Protocol work belongs exclusively to the SNI worker.
use super::{
    popup::{Act, Row},
    Button, DecBar, Open, Tray,
};
use crate::sni::{Action, MenuItem, Status};
use tiny_skia::Pixmap;
const PAGE: usize = 12;
fn title(s: &str) -> String {
    let s: String = s.chars().map(|c| if c.is_whitespace() { ' ' } else { c }).filter(|c| !c.is_control()).take(160).collect();
    if s.is_empty() {
        "Background app".into()
    } else {
        s
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum IndicatorPress {
    Ignored,
    Handled,
    Menu,
}
fn label(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().filter(|c| !c.is_control()).take(240).peekable();
    while let Some(c) = chars.next() {
        if c == '_' {
            if chars.peek() == Some(&'_') {
                chars.next();
                out.push('_');
            }
        } else {
            out.push(c);
        }
    }
    out
}
fn children<'a>(root: &'a [MenuItem], path: &[i32]) -> &'a [MenuItem] {
    let mut rows = root;
    for id in path {
        let Some(row) = rows.iter().find(|r| r.id == *id && r.visible && r.enabled) else { return root };
        rows = &row.children;
    }
    rows
}
impl DecBar {
    fn indicator_host_wanted(&self) -> bool {
        self.embedded || self.place().is_none_or(|p| p.part != super::vbar::Part::Head)
    }
    pub(super) fn ensure_indicator_host(&mut self) {
        if !self.indicator_host_wanted() {
            self.indicator_host = None;
            self.indicators = crate::sni::Snapshot::default();
            return;
        }
        if self.indicator_host.is_none() && !cfg!(test) {
            if let Some(wake) = self.waker.clone() {
                self.indicator_host = Some(crate::sni::Host::start(std::sync::Arc::new(move || wake.wake())));
            }
        }
    }
    pub(super) fn indicator_kinds(&self, limit: usize) -> Vec<Tray> {
        let active: Vec<_> = self.indicators.items.iter().filter(|i| i.status != Status::Passive).collect();
        if limit == 0 {
            return vec![];
        }
        if active.len() <= limit {
            return active.iter().map(|i| Tray::Indicator(i.key)).collect();
        }
        let mut kinds: Vec<_> = active.iter().take(limit - 1).map(|i| Tray::Indicator(i.key)).collect();
        kinds.push(Tray::Indicators);
        kinds
    }
    pub(super) fn indicator_tick(&mut self) -> bool {
        self.ensure_indicator_host();
        let Some(host) = &self.indicator_host else { return false };
        let next = host.snapshot();
        if self.indicators.generation == next.generation && self.indicators.connected == next.connected {
            return false;
        }
        self.indicators = next;
        if let Open::Popup(Tray::Indicator(key)) = self.open {
            if !self.indicators.items.iter().any(|i| i.key == key && i.status != Status::Passive) {
                self.close();
            }
        }
        true
    }
    fn indicator_send(&self, action: Action) -> bool {
        self.indicator_host.as_ref().is_some_and(|h| h.send(action))
    }
    pub(super) fn indicator_prepare(&mut self, key: u64, x: i32, y: i32) {
        self.indicator_path.clear();
        self.indicator_page = 0;
        self.indicator_anchor = (x.saturating_add(self.indicator_origin.0), y.saturating_add(self.indicator_origin.1));
        self.indicator_send(Action::OpenMenu { key, x: x.saturating_add(self.indicator_origin.0), y: y.saturating_add(self.indicator_origin.1) });
    }
    pub(super) fn indicator_press(&mut self, key: u64, button: Button, x: i32, y: i32) -> IndicatorPress {
        let Some(item) = self.indicators.items.iter().find(|i| i.key == key && i.status != Status::Passive) else { return IndicatorPress::Ignored };
        if button == Button::Right || (button == Button::Left && item.item_is_menu) {
            self.indicator_prepare(key, x, y);
            return IndicatorPress::Menu;
        }
        let (x, y) = (x.saturating_add(self.indicator_origin.0), y.saturating_add(self.indicator_origin.1));
        let action = if button == Button::Middle { Action::Secondary { key, x, y } } else { Action::Activate { key, x, y } };
        self.indicator_send(action);
        IndicatorPress::Handled
    }
    pub(super) fn indicator_scroll(&mut self, key: u64, dy: f32) -> bool {
        if !dy.is_finite() || dy == 0. {
            return false;
        }
        self.indicator_send(Action::Scroll { key, delta: if dy > 0. { -120 } else { 120 }, horizontal: false })
    }
    pub(super) fn indicator_draw(&self, pm: &mut Pixmap, key: u64, cx: f32, cy: f32, size: f32) {
        let Some(item) = self.indicators.items.iter().find(|i| i.key == key) else { return };
        if let Some(icon) = &item.icon {
            let k = size / icon.width().max(icon.height()).max(1) as f32;
            let (w, h) = (icon.width() as f32 * k, icon.height() as f32 * k);
            crate::gfx::blit_scaled(pm, icon, cx - w / 2., cy - h / 2., w, h, 1.);
        } else {
            // A missing icon is still an accessible target; no disk access here.
            crate::gfx::stroke_rounded(pm, cx - size / 2. + 2., cy - size / 2. + 2., size - 4., size - 4., 3., crate::theme::th().bar_text.color(0.8), 1.5);
        }
        if item.status == Status::NeedsAttention {
            crate::gfx::fill_circle(pm, cx + size / 2. - 1., cy - size / 2. + 1., 2.5, crate::theme::th().accent.color(1.));
        }
    }
    pub(super) fn indicator_overflow_draw(&self, pm: &mut Pixmap, cx: f32, cy: f32, size: f32) {
        for dx in [-0.3, 0., 0.3] {
            crate::gfx::fill_circle(pm, cx + dx * size, cy, 1.6, crate::theme::th().bar_text.color(0.9));
        }
    }
    pub(super) fn overflow_rows(&self) -> (Vec<Row>, f32, bool) {
        let mut rows = vec![Row::header("Background apps")];
        let items: Vec<_> = self.indicators.items.iter().filter(|i| i.status != Status::Passive).collect();
        let page = self.indicator_page.min(items.len().saturating_sub(1) / PAGE);
        for i in items.iter().skip(page * PAGE).take(PAGE) {
            rows.push(Row::item(&title(&i.title), Act::IndicatorOpen(i.key)).detail("›"));
        }
        self.indicator_pager(&mut rows, page, items.len());
        (rows, 300., false)
    }
    fn indicator_pager(&self, rows: &mut Vec<Row>, page: usize, count: usize) {
        if count <= PAGE {
            return;
        }
        rows.push(Row::sep());
        if page > 0 {
            rows.push(Row::item("Previous", Act::IndicatorPage(-1)));
        }
        if (page + 1) * PAGE < count {
            rows.push(Row::item("More…", Act::IndicatorPage(1)));
        }
    }
    pub(super) fn indicator_rows(&self, key: u64) -> (Vec<Row>, f32, bool) {
        let Some(item) = self.indicators.items.iter().find(|i| i.key == key) else { return (vec![Row::text("App is no longer available")], 300., false) };
        let mut rows = vec![Row::header(&title(&item.title))];
        if !self.indicator_path.is_empty() {
            rows.push(Row::item("‹ Back", Act::IndicatorBack));
        } else if !item.item_is_menu {
            rows.push(Row::item("Open app", Act::IndicatorActivate(key)));
        }
        if self.indicator_path.is_empty() && !item.tooltip.is_empty() && item.tooltip != item.title {
            rows.push(Row::text(&title(&item.tooltip)));
        }
        if let Some(menu) = &item.menu {
            let menu: Vec<_> = children(menu, &self.indicator_path).iter().filter(|r| r.visible).collect();
            let page = self.indicator_page.min(menu.len().saturating_sub(1) / PAGE);
            for row in menu.iter().skip(page * PAGE).take(PAGE) {
                if row.separator {
                    rows.push(Row::sep());
                    continue;
                }
                let submenu = row.submenu;
                let act = if submenu { Act::IndicatorSubmenu(row.id) } else { Act::IndicatorEvent { key, id: row.id } };
                rows.push(Row::item(&label(&row.label), act).enabled(row.enabled).checked(row.toggle == Some(true)).detail(if submenu { "›" } else { "" }));
            }
            if menu.is_empty() {
                rows.push(Row::text("No menu actions"));
            }
            self.indicator_pager(&mut rows, page, menu.len());
        } else {
            rows.push(Row::text(item.menu_error.as_deref().unwrap_or(if item.menu_loading { "Loading app menu…" } else { "App menu requested…" })));
        }
        (rows, 320., false)
    }
    pub(super) fn indicator_action(&mut self, act: &Act) -> bool {
        match *act {
            Act::IndicatorOpen(key) => {
                self.indicator_prepare(key, self.indicator_anchor.0.saturating_sub(self.indicator_origin.0), self.indicator_anchor.1.saturating_sub(self.indicator_origin.1));
                self.open = Open::Popup(Tray::Indicator(key));
            }
            Act::IndicatorActivate(key) => {
                self.indicator_send(Action::Activate { key, x: self.indicator_anchor.0, y: self.indicator_anchor.1 });
                self.close();
                return true;
            }
            Act::IndicatorEvent { key, id } => {
                self.indicator_send(Action::MenuEvent { key, id, timestamp: 0 });
                self.close();
                return true;
            }
            Act::IndicatorSubmenu(id) => {
                if let Open::Popup(Tray::Indicator(key)) = self.open {
                    self.indicator_send(Action::OpenSubmenu { key, id });
                }
                self.indicator_path.push(id);
                self.indicator_page = 0;
            }
            Act::IndicatorBack => {
                self.indicator_path.pop();
                self.indicator_page = 0;
            }
            Act::IndicatorPage(delta) => {
                self.indicator_change_page(delta);
            }
            _ => return false,
        }
        self.popup_hover = None;
        self.rebuild_popup();
        true
    }
    fn indicator_change_page(&mut self, delta: isize) {
        let count = match self.open {
            Open::Popup(Tray::Indicators) => self.indicators.items.iter().filter(|i| i.status != Status::Passive).count(),
            Open::Popup(Tray::Indicator(key)) => {
                self.indicators.items.iter().find(|i| i.key == key).and_then(|i| i.menu.as_ref()).map_or(0, |m| children(m, &self.indicator_path).iter().filter(|r| r.visible).count())
            }
            _ => 0,
        };
        self.indicator_page = self.indicator_page.saturating_add_signed(delta).min(count.saturating_sub(1) / PAGE);
    }
    pub(super) fn indicator_menu_scroll(&mut self, x: f32, y: f32, dy: f32) -> bool {
        if !dy.is_finite() || dy == 0. || !matches!(self.open, Open::Popup(Tray::Indicator(_) | Tray::Indicators)) {
            return false;
        }
        if !self.popup.as_ref().is_some_and(|p| x >= p.x && x < p.x + p.w && y >= p.y && y < p.y + p.h) {
            return false;
        }
        let before = self.indicator_page;
        self.indicator_change_page(if dy > 0. { 1 } else { -1 });
        if self.indicator_page != before {
            self.popup_hover = None;
            self.rebuild_popup();
        }
        true
    }
    pub(super) fn indicator_debug(&mut self, rects: &[super::IndicatorRect]) {
        if self.indicator_debug_file.is_none() {
            return;
        }
        let mut items = Vec::new();
        let mut overflow = None;
        for (kind, rect) in rects {
            match kind {
                Tray::Indicator(key) => {
                    if let Some(i) = self.indicators.items.iter().find(|i| i.key == *key) {
                        items.push(serde_json::json!({"key":key,"title":i.title,"status":format!("{:?}",i.status),"rect":[rect.0,rect.1,rect.2,rect.3],"icon_size":i.icon.as_ref().map(|p|[p.width(),p.height()])}));
                    }
                }
                Tray::Indicators => overflow = Some([rect.0, rect.1, rect.2, rect.3]),
                _ => {}
            }
        }
        let menu = if matches!(self.open, Open::Popup(Tray::Indicator(_) | Tray::Indicators)) {
            self.popup.as_ref().map(|p| {
                let rows:Vec<_>=p.rows.iter().enumerate().map(|(index,row)| {
                    let (y,h)=p.row_span(index);
                    let id=match row.act {Act::IndicatorEvent{id,..}|Act::IndicatorSubmenu(id)=>Some(id),_=>None};
                    serde_json::json!({"id":id,"index":index,"label":row.label,"enabled":row.enabled,"checked":row.checked,"submenu":matches!(row.act,Act::IndicatorSubmenu(_)|Act::IndicatorOpen(_)),"rect":[p.x,p.y+y,p.w,h]})
                }).collect();
                let key=if let Open::Popup(Tray::Indicator(k))=self.open {Some(k)}else{None};
                serde_json::json!({"key":key,"rows":rows})
            })
        } else {
            None
        };
        let text = serde_json::json!({"origin":[self.indicator_origin.0,self.indicator_origin.1],"items":items,"overflow":overflow,"menu":menu,"connected":self.indicators.connected}).to_string();
        if let Some((path, last)) = &mut self.indicator_debug_file {
            if *last != text {
                let _ = std::fs::write(path, &text);
                *last = text;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        menubar::active::StaticApp,
        sni::{Item, Snapshot},
    };
    use std::sync::Arc;
    fn item(key: u64, status: Status) -> Item {
        Item { key, title: format!("App_{key}"), tooltip: String::new(), status, icon_name: String::new(), icon: None, item_is_menu: false, menu: None, menu_loading: false, menu_error: None }
    }
    fn bar() -> DecBar {
        DecBar::new(Box::new(StaticApp("Test".into())))
    }
    fn row(id: i32, label: &str) -> MenuItem {
        MenuItem { id, label: label.into(), enabled: true, visible: true, separator: false, toggle: None, children: vec![], submenu: false, icon_name: String::new() }
    }
    #[test]
    fn allocation_keeps_stable_keys_passive_hidden_and_overflow_reachable() {
        let mut b = bar();
        b.indicators = Snapshot { items: vec![item(3, Status::Active), item(4, Status::Passive), item(9, Status::NeedsAttention), item(12, Status::Active)], ..Snapshot::default() };
        assert_eq!(b.indicator_kinds(4), vec![Tray::Indicator(3), Tray::Indicator(9), Tray::Indicator(12)]);
        assert_eq!(b.indicator_kinds(2), vec![Tray::Indicator(3), Tray::Indicators]);
        assert_eq!(b.indicator_kinds(1), vec![Tray::Indicators]);
        assert!(b.indicator_kinds(0).is_empty());
        assert_eq!(b.overflow_rows().0[1].label, "App_3");
        assert!(b.indicator_host.is_none());
    }
    #[test]
    fn context_and_overflow_anchors_survive_popup_surface_repositioning() {
        let mut b = bar();
        b.indicator_origin = (1920, 850);
        b.indicator_prepare(1, 280, 25);
        assert_eq!(b.indicator_anchor, (2200, 875));
        b.indicator_origin = (1920, 500);
        b.indicator_action(&Act::IndicatorOpen(2));
        assert_eq!(b.indicator_anchor, (2200, 875));
    }
    #[test]
    fn typed_menus_preserve_hierarchy_disabled_checks_and_pagination() {
        let mut b = bar();
        let mut i = item(8, Status::Active);
        let mut menu: Vec<_> = (0..20).map(|id| row(id, &format!("Action {id}"))).collect();
        menu[1].enabled = false;
        menu[2].toggle = Some(true);
        menu[3].visible = false;
        menu[4].children = vec![row(100, "_Nested __ literal")];
        menu[4].submenu = true;
        i.menu = Some(Arc::new(menu));
        b.indicators.items.push(i);
        let (rows, _, _) = b.indicator_rows(8);
        assert!(!rows.iter().find(|r| r.label == "Action 1").unwrap().enabled);
        assert!(rows.iter().find(|r| r.label == "Action 2").unwrap().checked);
        assert!(!rows.iter().any(|r| r.label == "Action 3"));
        assert!(rows.iter().any(|r| r.act == Act::IndicatorPage(1)));
        b.indicator_path = vec![4];
        let (rows, _, _) = b.indicator_rows(8);
        assert!(rows.iter().any(|r| r.act == Act::IndicatorBack));
        assert!(rows.iter().any(|r| r.label == "Nested _ literal" && r.act == Act::IndicatorEvent { key: 8, id: 100 }));
    }
    #[test]
    fn narrow_horizontal_bar_preserves_background_app_access_and_hides_overlap_titles() {
        let mut b = bar();
        b.width = 800;
        b.indicators.items = (0..32).map(|k| item(k, Status::Active)).collect();
        let tray = b.tray();
        assert!(tray.iter().any(|(k, _)| *k == Tray::Indicators));
        for (kind, span) in tray {
            if matches!(kind, Tray::Indicator(_) | Tray::Indicators) {
                assert!(b.title_hit(span.x + span.w / 2.).is_none());
            }
        }
    }
    #[test]
    fn painting_cached_icon_preserves_aspect_ratio_and_never_starts_a_host() {
        let mut b = bar();
        let mut i = item(1, Status::Active);
        let mut icon = Pixmap::new(20, 10).unwrap();
        icon.fill(tiny_skia::Color::from_rgba8(240, 60, 90, 255));
        i.icon = Some(Arc::new(icon));
        b.indicators.items.push(i);
        let mut pm = Pixmap::new(40, 40).unwrap();
        b.indicator_draw(&mut pm, 1, 20., 20., 20.);
        assert_eq!(&pm.data()[(20 * 40 + 20) * 4..(20 * 40 + 20) * 4 + 4], &[240, 60, 90, 255]);
        assert_eq!(pm.pixel(20, 12).unwrap().alpha(), 0);
        assert!(b.indicator_host.is_none());
    }
}
