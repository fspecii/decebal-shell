//! Tabs: each tab owns its navigation model (history, columns, selection) and explicit
//! layout. Only the active tab's model is live in `Ui::model`; switching swaps models and
//! is allowed only while idle, so no pending request or operation can cross tabs.
use super::Ui;
use decfiles::model::{Intent, Model};
use gtk::prelude::*;
use std::{path::Path, rc::Rc};

pub const MAX_TABS: usize = 16;

#[derive(Default)]
pub struct Tab {
    /// Empty placeholder while this tab is active (its model is in `Ui::model`).
    pub model: Model,
    pub columns: bool,
}

fn label_for(path: Option<&Path>) -> String {
    path.map_or_else(|| "New tab".to_string(), |path| path.file_name().map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into_owned()))
}

impl Ui {
    /// Open the current folder in a new tab, like Ctrl+T in GNOME Files.
    pub fn new_tab(self: &Rc<Self>) {
        self.drop_quiet_refresh();
        let Some(path) = self.model.borrow().current().map(Path::to_path_buf) else {
            return;
        };
        if !self.can_start_dialog() || self.tabs.borrow().len() >= MAX_TABS {
            return;
        }
        self.park_active();
        let columns = self.columns.get();
        let index = {
            let mut tabs = self.tabs.borrow_mut();
            let index = self.active_tab.get() + 1;
            tabs.insert(index, Tab { model: Model::default(), columns });
            index
        };
        self.active_tab.set(index);
        self.render();
        self.navigate(path, Intent::Visit);
    }

    pub fn switch_tab(self: &Rc<Self>, index: usize) {
        self.drop_quiet_refresh();
        if index == self.active_tab.get() || index >= self.tabs.borrow().len() || !self.can_start_dialog() {
            return;
        }
        self.park_active();
        let tab = std::mem::take(&mut self.tabs.borrow_mut()[index]);
        self.model.replace(tab.model);
        self.active_tab.set(index);
        self.ops.notice.borrow_mut().take();
        self.ops.active.replace(None);
        self.set_columns(tab.columns);
        self.render();
        // The tab may have missed changes while inactive; monitor and refresh it now.
        self.watch_current();
        self.refresh_quietly();
    }

    pub fn cycle_tab(self: &Rc<Self>, forward: bool) {
        let count = self.tabs.borrow().len();
        if count > 1 {
            let active = self.active_tab.get();
            self.switch_tab(if forward { (active + 1) % count } else { (active + count - 1) % count });
        }
    }

    /// Close a tab; closing the last tab closes the window (and its usual confirmation).
    pub fn close_tab(self: &Rc<Self>, index: usize) {
        self.drop_quiet_refresh();
        let count = self.tabs.borrow().len();
        if count <= 1 {
            self.window.close();
            return;
        }
        if index >= count || !self.can_start_dialog() {
            return;
        }
        let active = self.active_tab.get();
        if index == active {
            self.switch_tab(if index + 1 < count { index + 1 } else { index - 1 });
        }
        self.tabs.borrow_mut().remove(index);
        if index < self.active_tab.get() {
            self.active_tab.set(self.active_tab.get() - 1);
        }
        self.render();
    }

    /// Move the live model into the active tab's slot.
    fn park_active(&self) {
        let model = std::mem::take(&mut *self.model.borrow_mut());
        let mut tabs = self.tabs.borrow_mut();
        let slot = &mut tabs[self.active_tab.get()];
        slot.model = model;
        slot.columns = self.columns.get();
    }

    fn tab_path(&self, index: usize) -> Option<std::path::PathBuf> {
        if index == self.active_tab.get() {
            self.model.borrow().current().map(Path::to_path_buf)
        } else {
            self.tabs.borrow().get(index).and_then(|tab| tab.model.current().map(Path::to_path_buf))
        }
    }

    /// Rebuild the tab bar (bounded by MAX_TABS); hidden with a single tab.
    pub fn render_tabs(self: &Rc<Self>) {
        while let Some(child) = self.tab_bar.first_child() {
            self.tab_bar.remove(&child);
        }
        let count = self.tabs.borrow().len();
        self.tab_bar.set_visible(count > 1);
        if count <= 1 {
            return;
        }
        // Background refreshes are superseded by tab actions, so only an operation or a
        // dialog (both of which end with a re-render) disables the tab controls.
        let idle = self.ops.running.borrow().is_none() && self.ops.dialog.borrow().is_none();
        for index in 0..count {
            let path = self.tab_path(index);
            let name = label_for(path.as_deref());
            let group = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            group.add_css_class("linked");
            // Equal-width tabs with ellipsized names, as in Explorer and Finder.
            let title = gtk::Label::new(Some(&name));
            title.set_ellipsize(gtk::pango::EllipsizeMode::End);
            title.set_width_chars(16);
            title.set_max_width_chars(16);
            let tab = gtk::ToggleButton::new();
            tab.set_child(Some(&title));
            tab.set_active(index == self.active_tab.get());
            tab.set_sensitive(idle || index == self.active_tab.get());
            if let Some(path) = &path {
                tab.set_tooltip_text(Some(&path.to_string_lossy()));
            }
            tab.update_property(&[gtk::accessible::Property::Label(&format!("Tab {}: {name}", index + 1))]);
            let close = gtk::Button::from_icon_name("window-close-symbolic");
            close.set_tooltip_text(Some("Close tab"));
            close.set_sensitive(idle);
            close.update_property(&[gtk::accessible::Property::Label(&format!("Close tab {name}"))]);
            let weak = Rc::downgrade(self);
            tab.connect_clicked(move |button| {
                if let Some(ui) = weak.upgrade() {
                    if index != ui.active_tab.get() {
                        ui.switch_tab(index);
                    } else {
                        // A toggle must not untoggle the active tab.
                        button.set_active(true);
                    }
                }
            });
            let weak = Rc::downgrade(self);
            close.connect_clicked(move |_| {
                if let Some(ui) = weak.upgrade() {
                    ui.close_tab(index);
                }
            });
            group.append(&tab);
            group.append(&close);
            self.tab_bar.append(&group);
        }
        let add = gtk::Button::from_icon_name("list-add-symbolic");
        add.set_tooltip_text(Some("New tab"));
        add.update_property(&[gtk::accessible::Property::Label("New tab")]);
        add.set_sensitive(idle && count < MAX_TABS);
        let weak = Rc::downgrade(self);
        add.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.new_tab();
            }
        });
        self.tab_bar.append(&add);
    }

    pub fn tabs_evidence(&self) -> serde_json::Value {
        let count = self.tabs.borrow().len();
        let paths: Vec<_> = (0..count).map(|index| self.tab_path(index).map(|path| path.to_string_lossy().into_owned())).collect();
        let layouts: Vec<_> = (0..count)
            .map(|index| if index == self.active_tab.get() { self.columns.get() } else { self.tabs.borrow()[index].columns })
            .map(|columns| if columns { "columns" } else { "details" })
            .collect();
        serde_json::json!({"active":self.active_tab.get(), "paths":paths, "layouts":layouts})
    }
}
