use super::Ui;
use decfiles::{
    model::Intent,
    operations::{self, Action, Failure, Outcome},
};
use gio::prelude::*;
use gtk::prelude::*;
use std::{
    cell::{Cell, RefCell},
    path::{Path, PathBuf},
    rc::Rc,
};

pub struct Controls {
    pub create: gtk::Button,
    pub rename: gtk::Button,
    pub trash: gtk::Button,
    pub cancel: gtk::Button,
    pub active: RefCell<Option<PathBuf>>,
    running: RefCell<Option<Running>>,
    dialog: RefCell<Option<gtk::Dialog>>,
    next_id: Cell<u64>,
    select_after_refresh: RefCell<Option<PathBuf>>,
    notice: RefCell<Option<String>>,
    last_result: RefCell<Option<serde_json::Value>>,
    allow_close: Cell<bool>,
}
struct Running {
    id: u64,
    cancel: gio::Cancellable,
}

impl Controls {
    pub fn new() -> Self {
        Self {
            create: gtk::Button::with_label("New folder"),
            rename: gtk::Button::with_label("Rename"),
            trash: gtk::Button::with_label("Move to Trash"),
            cancel: gtk::Button::with_label("Cancel operation"),
            active: RefCell::new(None),
            running: RefCell::new(None),
            dialog: RefCell::new(None),
            next_id: Cell::new(0),
            select_after_refresh: RefCell::new(None),
            notice: RefCell::new(None),
            last_result: RefCell::new(None),
            allow_close: Cell::new(false),
        }
    }
    pub fn bar(&self) -> gtk::Box {
        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        bar.set_margin_top(8);
        bar.set_margin_start(12);
        bar.set_margin_end(12);
        for button in [&self.create, &self.rename, &self.trash, &self.cancel] {
            bar.append(button);
        }
        self.cancel.set_visible(false);
        bar
    }
    pub fn evidence(&self) -> serde_json::Value {
        use std::os::unix::ffi::OsStrExt;
        let running = self.running.borrow();
        serde_json::json!({"busy":running.is_some(), "id":running.as_ref().map(|run|run.id),
            "cancel_requested":running.as_ref().is_some_and(|run|run.cancel.is_cancelled()),
            "dialog":self.dialog.borrow().as_ref().and_then(|dialog|dialog.title()).map(|title|title.to_string()),
            "selected":self.active.borrow().as_ref().map(|path|path.to_string_lossy().into_owned()),
            "selected_bytes":self.active.borrow().as_ref().map(|path|path.as_os_str().as_bytes().to_vec()),
            "last_result":&*self.last_result.borrow()})
    }
    pub fn is_busy(&self) -> bool {
        self.running.borrow().is_some()
    }
    pub fn clear_notice(&self) {
        self.notice.borrow_mut().take();
    }
    pub fn notice(&self) -> Option<String> {
        self.notice.borrow().clone()
    }
}

impl Ui {
    pub fn connect_operations(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.ops.create.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.name_dialog(false);
            }
        });
        let weak = Rc::downgrade(self);
        self.ops.rename.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.name_dialog(true);
            }
        });
        let weak = Rc::downgrade(self);
        self.ops.trash.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.trash_dialog();
            }
        });
        let weak = Rc::downgrade(self);
        self.ops.cancel.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                if let Some(run) = ui.ops.running.borrow().as_ref() {
                    run.cancel.cancel();
                }
                ui.status.set_text("Cancelling… A change that has already finished may remain.");
                ui.update_operation_controls();
                ui.dump();
            }
        });
        let weak = Rc::downgrade(self);
        self.window.connect_close_request(move |_| {
            let Some(ui) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if ui.ops.running.borrow().is_some() && !ui.ops.allow_close.get() {
                ui.close_operation_dialog();
                return glib::Propagation::Stop;
            }
            let dialog = ui.ops.dialog.borrow_mut().take();
            if let Some(dialog) = dialog {
                dialog.destroy();
            }
            glib::Propagation::Proceed
        });
        self.update_operation_controls();
    }

    fn close_operation_dialog(self: &Rc<Self>) {
        if let Some(dialog) = self.ops.dialog.borrow().as_ref() {
            dialog.present();
            return;
        }
        let dialog = gtk::Dialog::builder().transient_for(&self.window).modal(true).destroy_with_parent(true).title("Close Files during an operation?").default_width(490).build();
        let keep = dialog.add_button("Keep Open", gtk::ResponseType::Cancel);
        dialog.add_button("Cancel operation and close", gtk::ResponseType::Accept);
        dialog.set_default_response(gtk::ResponseType::Cancel);
        let label = gtk::Label::new(Some(
            "An operation is still running.\n\nCancel it and close Files? A change that has already finished may remain. Check the folder or Trash the next time you open Files.\n\nChoose Keep Open to wait for the result.",
        ));
        label.set_wrap(true);
        label.set_xalign(0.0);
        label.set_margin_top(18);
        label.set_margin_bottom(18);
        label.set_margin_start(18);
        label.set_margin_end(18);
        dialog.content_area().append(&label);
        self.ops.dialog.replace(Some(dialog.clone()));
        self.dump();
        let weak = Rc::downgrade(self);
        dialog.connect_response(move |dialog, response| {
            let Some(ui) = weak.upgrade() else {
                dialog.destroy();
                return;
            };
            ui.ops.dialog.borrow_mut().take();
            dialog.destroy();
            if response == gtk::ResponseType::Accept {
                if let Some(run) = ui.ops.running.borrow().as_ref() {
                    run.cancel.cancel();
                }
                ui.ops.allow_close.set(true);
                ui.dump();
                ui.window.close();
            } else {
                ui.status.set_text("Window kept open. The operation is still running unless its outcome is shown.");
                ui.update_operation_controls();
                ui.dump();
            }
        });
        dialog.present();
        keep.grab_focus();
    }

    pub fn navigation_allowed(&self) -> bool {
        if self.ops.running.borrow().is_some() {
            self.status.set_text("Finish or cancel the current operation before navigating.");
            return false;
        }
        if self.ops.dialog.borrow().is_some() {
            return false;
        }
        self.ops.notice.borrow_mut().take();
        true
    }

    pub fn update_operation_controls(&self) {
        let model = self.model.borrow();
        let selected =
            self.ops.active.borrow().as_ref().is_some_and(|path| model.columns.iter().any(|column| column.selected.as_ref() == Some(path) && column.entries.iter().any(|entry| &entry.path == path)));
        let running = self.ops.running.borrow();
        let idle = running.is_none() && self.ops.dialog.borrow().is_none() && model.pending.is_none();
        self.ops.create.set_sensitive(idle && model.current().is_some());
        self.ops.rename.set_sensitive(idle && selected);
        self.ops.trash.set_sensitive(idle && selected);
        self.ops.cancel.set_visible(running.is_some());
        self.ops.cancel.set_sensitive(running.as_ref().is_some_and(|run| !run.cancel.is_cancelled()));
        self.back.set_sensitive(running.is_none() && model.can_back());
        self.forward.set_sensitive(running.is_none() && model.can_forward());
        self.up.set_sensitive(running.is_none() && model.current().and_then(Path::parent).is_some());
        self.address.set_sensitive(running.is_none());
        self.layout.set_sensitive(running.is_none());
    }

    pub fn reconcile_operation_selection(&self) {
        let mut active = self.ops.active.borrow_mut();
        let model = self.model.borrow();
        if active.as_ref().is_some_and(|path| model.columns.iter().any(|column| column.selected.as_ref() == Some(path) && column.entries.iter().any(|entry| &entry.path == path))) {
            return;
        }
        *active = model.columns.last().and_then(|column| column.selected.clone());
    }

    pub fn operation_refreshed(&self) {
        if let Some(path) = self.ops.select_after_refresh.borrow_mut().take() {
            let mut model = self.model.borrow_mut();
            if model.error.is_none() {
                if let Some(column) = model.columns.last_mut() {
                    column.selected = column.entries.iter().any(|entry| entry.path == path).then_some(path.clone());
                    self.ops.active.replace(column.selected.clone());
                }
            }
        }
    }

    fn can_start_dialog(&self) -> bool {
        self.ops.running.borrow().is_none() && self.ops.dialog.borrow().is_none() && self.model.borrow().pending.is_none()
    }

    pub fn name_dialog(self: &Rc<Self>, rename: bool) {
        if !self.can_start_dialog() {
            return;
        }
        let source = if rename { self.ops.active.borrow().clone() } else { self.model.borrow().current().map(Path::to_path_buf) };
        let Some(source) = source else {
            return;
        };
        let title = if rename { "Rename item" } else { "Create folder" };
        let verb = if rename { "Rename" } else { "Create" };
        let dialog = gtk::Dialog::builder().transient_for(&self.window).modal(true).destroy_with_parent(true).title(title).default_width(460).build();
        dialog.add_button("Cancel", gtk::ResponseType::Cancel);
        dialog.add_button(verb, gtk::ResponseType::Accept);
        dialog.set_default_response(gtk::ResponseType::Accept);
        let body = dialog.content_area();
        body.set_spacing(10);
        body.set_margin_top(18);
        body.set_margin_bottom(18);
        body.set_margin_start(18);
        body.set_margin_end(18);
        let location = if rename {
            format!("Item: {}\nLocation: {}", source.file_name().unwrap_or_default().to_string_lossy(), source.parent().unwrap_or(Path::new("/")).display())
        } else {
            format!("Create a folder in:\n{}", source.display())
        };
        let label = gtk::Label::new(Some(&location));
        label.set_wrap(true);
        label.set_xalign(0.0);
        label.set_selectable(true);
        body.append(&label);
        let entry = gtk::Entry::builder().placeholder_text("Name").activates_default(true).hexpand(true).build();
        entry.update_property(&[gtk::accessible::Property::Label(if rename { "New name" } else { "Folder name" })]);
        if rename {
            entry.set_text(source.file_name().and_then(|name| name.to_str()).unwrap_or(""));
        }
        body.append(&entry);
        let validation = gtk::Label::new(None);
        validation.set_wrap(true);
        validation.set_xalign(0.0);
        validation.add_css_class("error");
        body.append(&validation);
        let original = source.clone();
        let check = move |name: &str| {
            operations::validate_name(name)?;
            if rename && original.file_name().is_some_and(|old| old == name) {
                return Err("Enter a different name.".into());
            }
            Ok::<(), String>(())
        };
        let accept = dialog.widget_for_response(gtk::ResponseType::Accept).unwrap();
        let update = {
            let validation = validation.clone();
            let accept = accept.clone();
            move |entry: &gtk::Entry| {
                let result = check(entry.text().as_str());
                accept.set_sensitive(result.is_ok());
                validation.set_text(result.err().as_deref().unwrap_or(""));
            }
        };
        update(&entry);
        entry.connect_changed(update);
        self.ops.dialog.replace(Some(dialog.clone()));
        self.update_operation_controls();
        self.dump();
        let weak = Rc::downgrade(self);
        let field = entry.clone();
        dialog.connect_response(move |dialog, response| {
            let Some(ui) = weak.upgrade() else {
                dialog.destroy();
                return;
            };
            ui.ops.dialog.borrow_mut().take();
            dialog.destroy();
            if response == gtk::ResponseType::Accept {
                let name = field.text().to_string();
                let action = if rename { Action::Rename { source: source.clone(), name } } else { Action::CreateFolder { parent: source.clone(), name } };
                ui.start_operation(action);
            } else {
                ui.status.set_text("Cancelled. No operation was started.");
                ui.update_operation_controls();
                ui.dump();
            }
        });
        dialog.present();
        entry.grab_focus();
        entry.select_region(0, -1);
    }

    pub fn trash_dialog(self: &Rc<Self>) {
        if !self.can_start_dialog() {
            return;
        }
        let Some(source) = self.ops.active.borrow().clone() else {
            return;
        };
        let dialog = gtk::Dialog::builder().transient_for(&self.window).modal(true).destroy_with_parent(true).title("Move item to Trash?").default_width(480).build();
        let cancel_button = dialog.add_button("Cancel", gtk::ResponseType::Cancel);
        dialog.add_button("Move to Trash", gtk::ResponseType::Accept);
        dialog.set_default_response(gtk::ResponseType::Cancel);
        let label = gtk::Label::new(Some(&format!(
            "Move this item to Trash?\n\nItem: {}\nLocation: {}",
            source.file_name().unwrap_or_default().to_string_lossy(),
            source.parent().unwrap_or(Path::new("/")).display()
        )));
        label.set_wrap(true);
        label.set_xalign(0.0);
        label.set_selectable(true);
        label.set_margin_top(18);
        label.set_margin_bottom(18);
        label.set_margin_start(18);
        label.set_margin_end(18);
        dialog.content_area().append(&label);
        self.ops.dialog.replace(Some(dialog.clone()));
        self.update_operation_controls();
        self.dump();
        let weak = Rc::downgrade(self);
        dialog.connect_response(move |dialog, response| {
            let Some(ui) = weak.upgrade() else {
                dialog.destroy();
                return;
            };
            ui.ops.dialog.borrow_mut().take();
            dialog.destroy();
            if response == gtk::ResponseType::Accept {
                ui.start_operation(Action::Trash { source: source.clone() });
            } else {
                ui.status.set_text("Cancelled. Nothing was moved to Trash.");
                ui.update_operation_controls();
                ui.dump();
            }
        });
        dialog.present();
        cancel_button.grab_focus();
    }

    fn start_operation(self: &Rc<Self>, action: Action) {
        if !self.can_start_dialog() {
            return;
        }
        if let Err(error) = action.validate() {
            self.model.borrow_mut().error = Some(error.message);
            self.render();
            return;
        }
        let id = self.ops.next_id.get().wrapping_add(1);
        self.ops.next_id.set(id);
        let cancel = gio::Cancellable::new();
        self.ops.running.replace(Some(Running { id, cancel: cancel.clone() }));
        self.ops.notice.borrow_mut().take();
        self.model.borrow_mut().error = None;
        self.error.set_visible(false);
        self.status.set_text("Working… You can cancel this operation.");
        self.update_operation_controls();
        self.dump();
        let parent = action.parent();
        let weak = Rc::downgrade(self);
        operations::run(action, &cancel, move |result| {
            if let Some(ui) = weak.upgrade() {
                ui.operation_finished(id, parent, result);
            }
        });
    }

    fn operation_finished(self: &Rc<Self>, id: u64, parent: PathBuf, result: Result<Outcome, Failure>) {
        if self.ops.running.borrow().as_ref().map(|run| run.id) != Some(id) {
            return;
        }
        self.ops.running.borrow_mut().take();
        // The operation can finish while a close confirmation is visible. Keep the
        // window open, remove that now-unnecessary prompt, and show the real outcome.
        let dialog = self.ops.dialog.borrow_mut().take();
        if let Some(dialog) = dialog {
            dialog.destroy();
        }
        match result {
            Ok(outcome) => {
                self.ops.last_result.replace(Some(serde_json::json!({"id":id,"status":"success","message":outcome.message})));
                self.refresh_operation(outcome.parent, outcome.selected, outcome.message);
            }
            Err(error) => {
                self.ops.last_result.replace(Some(serde_json::json!({"id":id,"status":"error","kind":format!("{:?}",error.kind),"message":error.message,"may_have_changed":error.may_have_changed})));
                if error.may_have_changed {
                    self.refresh_operation(parent, None, error.message);
                } else {
                    self.model.borrow_mut().error = Some(error.message);
                    self.render();
                }
            }
        }
    }

    fn refresh_operation(self: &Rc<Self>, parent: PathBuf, selected: Option<PathBuf>, notice: String) {
        let same_folder = self.model.borrow().current() == Some(parent.as_path());
        self.ops.select_after_refresh.replace(selected);
        self.navigate(parent, if same_folder { Intent::Refresh } else { Intent::Visit });
        self.ops.notice.replace(Some(notice));
        self.dump();
    }
}
