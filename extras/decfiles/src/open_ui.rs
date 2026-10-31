//! Open and Open With. The footer buttons, the row context menu, Ctrl+O / Ctrl+Shift+O and
//! Enter/double click all end in `open_entry`. Every request is a `decfiles::opening`
//! request with an identity: it is cancelled by a newer request, a changed selection,
//! navigation, a search change or closing, and a completion is acted on only while it is
//! still the current request for the same folder, tab, search and exact selected path.
//! Open With uses the native GTK application chooser and launches the chosen application
//! itself; it never records a default or "last used" handler.
// GtkDialog and GtkAppChooserWidget are deprecated since GTK 4.10 but remain the supported
// GTK 4 application chooser (GtkFileLauncher's always-ask needs GTK 4.12 bindings).
#![allow(deprecated)]
use super::Ui;
use decfiles::{
    model::{Entry, Intent},
    opening::{self, Probe, Purpose, Scope, Stage, Tracker},
};
use gio::prelude::*;
use gtk::prelude::*;
use std::{cell::RefCell, path::Path, rc::Rc};

/// Row context menu: (position in the view's model, cell widget, x, y) of a secondary click.
pub type RowMenu = Rc<dyn Fn(u32, &gtk::Widget, f64, f64)>;
/// A view's selection, available to its factories only after the view is built.
pub type SelectionSlot = Rc<RefCell<Option<gtk::SingleSelection>>>;

pub struct Controls {
    pub open: gtk::Button,
    pub open_with: gtk::Button,
    open_action: gio::SimpleAction,
    open_with_action: gio::SimpleAction,
    pub(crate) tracker: RefCell<Tracker>,
    menu: RefCell<Option<gtk::PopoverMenu>>,
    /// The selected search result (taken from the result store), used only by explicit Open.
    pub(crate) search_selected: RefCell<Option<Entry>>,
}

impl Controls {
    pub fn new() -> Self {
        let open = gtk::Button::with_label("Open");
        open.set_tooltip_text(Some("Open the selected file with its default application (Ctrl+O)"));
        open.update_property(&[gtk::accessible::Property::Label("Open the selected file")]);
        let open_with = gtk::Button::with_label("Open With…");
        open_with.set_tooltip_text(Some("Choose an application to open the selected file (Ctrl+Shift+O)"));
        open_with.update_property(&[gtk::accessible::Property::Label("Open the selected file with another application")]);
        Self {
            open,
            open_with,
            open_action: gio::SimpleAction::new("open", None),
            open_with_action: gio::SimpleAction::new("open-with", None),
            tracker: RefCell::new(Tracker::default()),
            menu: RefCell::new(None),
            search_selected: RefCell::new(None),
        }
    }

    /// Appended after the clipboard buttons so earlier controls keep their positions.
    pub fn append_to(&self, bar: &gtk::Box) {
        let separator = gtk::Separator::new(gtk::Orientation::Vertical);
        separator.set_margin_start(6);
        separator.set_margin_end(6);
        bar.append(&separator);
        bar.append(&self.open);
        bar.append(&self.open_with);
    }
}

fn leaf(path: &Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into_owned())
}

impl Ui {
    pub fn connect_open(self: &Rc<Self>) {
        let group = gio::SimpleActionGroup::new();
        group.add_action(&self.opener.open_action);
        group.add_action(&self.opener.open_with_action);
        self.window.insert_action_group("fileopen", Some(&group));
        let weak = Rc::downgrade(self);
        self.opener.open.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.open_selected(Purpose::Default);
            }
        });
        let weak = Rc::downgrade(self);
        self.opener.open_with.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.open_selected(Purpose::Chooser);
            }
        });
        let weak = Rc::downgrade(self);
        self.opener.open_action.connect_activate(move |_, _| {
            if let Some(ui) = weak.upgrade() {
                ui.open_selected(Purpose::Default);
            }
        });
        let weak = Rc::downgrade(self);
        self.opener.open_with_action.connect_activate(move |_, _| {
            if let Some(ui) = weak.upgrade() {
                ui.open_selected(Purpose::Chooser);
            }
        });
        self.update_open_controls();
    }

    /// The item Open acts on: the model-validated selection, or the selected search result.
    fn open_target(&self) -> Option<Entry> {
        if self.is_searching() {
            return self.opener.search_selected.borrow().clone();
        }
        let path = self.ops.active.borrow().clone()?;
        let model = self.model.borrow();
        let column = model.columns.iter().find(|column| column.selected.as_ref() == Some(&path))?;
        column.entries.iter().find(|entry| entry.path == path).cloned()
    }

    fn open_scope(&self) -> Scope {
        Scope { folder: self.model.borrow().current().map(Path::to_path_buf), tab: self.active_tab.get(), search: self.search.borrow().as_ref().map(|run| run.id) }
    }

    pub fn update_open_controls(&self) {
        let target = self.open_target();
        let idle = self.ops.running.borrow().is_none() && self.ops.dialog.borrow().is_none() && !self.ops.paste_reading.get() && (self.model.borrow().pending.is_none() || self.pending_is_quiet());
        let any = idle && target.is_some();
        let file = idle && target.as_ref().is_some_and(|entry| !entry.directory);
        self.opener.open.set_sensitive(any);
        self.opener.open_action.set_enabled(any);
        self.opener.open_with.set_sensitive(file);
        self.opener.open_with_action.set_enabled(file);
    }

    pub fn open_evidence(&self) -> serde_json::Value {
        use std::os::unix::ffi::OsStrExt;
        let mut value = self.opener.tracker.borrow().evidence();
        value["open_enabled"] = self.opener.open.is_sensitive().into();
        value["open_with_enabled"] = self.opener.open_with.is_sensitive().into();
        value["search_selected"] = match self.opener.search_selected.borrow().as_ref() {
            Some(entry) => serde_json::json!({"path":entry.path.to_string_lossy(), "path_bytes":entry.path.as_os_str().as_bytes()}),
            None => serde_json::Value::Null,
        };
        value
    }

    /// Drop the current request; its pending GIO operations are cancelled.
    pub fn cancel_open(&self, message: &str) {
        let token = self.opener.tracker.borrow_mut().cancel(message);
        if let Some(token) = token {
            token.cancel();
        }
    }

    /// A selection other than the exact target supersedes the request.
    pub fn open_selection_changed(&self, selected: Option<&Path>) {
        let token = self.opener.tracker.borrow_mut().cancel_unless_target(selected, "The selection changed, so nothing was opened.");
        if let Some(token) = token {
            token.cancel();
        }
    }

    /// Search results keep Enter = reveal; this only tracks which result explicit Open acts on.
    pub fn search_selection_changed(&self, entry: Option<Entry>) {
        self.open_selection_changed(entry.as_ref().map(|entry| entry.path.as_path()));
        self.opener.search_selected.replace(entry);
        self.update_open_controls();
        self.dump();
    }

    /// Footer button, menu item or shortcut.
    pub fn open_selected(self: &Rc<Self>, purpose: Purpose) {
        self.drop_quiet_refresh();
        match self.open_target() {
            Some(entry) => self.open_entry(entry, purpose),
            None => self.status.set_text("Select a file first."),
        }
    }

    /// Open `entry`: a folder is navigated to; a file goes through the asynchronous
    /// probe -> judge -> launch (or chooser) pipeline.
    pub fn open_entry(self: &Rc<Self>, entry: Entry, purpose: Purpose) {
        if entry.directory {
            if purpose == Purpose::Chooser {
                self.status.set_text("Open With is for files. Press Enter to open a folder.");
            } else {
                self.navigate(entry.path, Intent::Visit);
            }
            return;
        }
        self.drop_quiet_refresh();
        if self.ops.running.borrow().is_some() {
            self.status.set_text("Finish or cancel the current operation before opening a file.");
            return;
        }
        if self.ops.dialog.borrow().is_some() {
            return;
        }
        if self.ops.paste_reading.get() {
            self.status.set_text("Reading the clipboard… Opening resumes when the paste starts or fails.");
            return;
        }
        if self.model.borrow().pending.is_some() {
            self.status.set_text("Wait for the folder to finish loading before opening a file.");
            return;
        }
        // The launch target is always the validated selection, never whatever a view shows.
        if self.open_target().is_none_or(|target| target.path != entry.path) {
            self.status.set_text("Select the file first.");
            return;
        }
        let scope = self.open_scope();
        let (pending, replaced) = self.opener.tracker.borrow_mut().begin(entry.path.clone(), purpose, scope);
        if let Some(replaced) = replaced {
            replaced.cancel();
        }
        self.model.borrow_mut().error = None;
        self.error.set_visible(false);
        self.ops.clear_notice();
        self.status.set_text(&format!("Opening “{}”…", entry.name));
        self.update_operation_controls();
        self.dump();
        let (weak, id) = (Rc::downgrade(self), pending.id);
        probe(&pending.path, &pending.cancel, move |result| {
            if let Some(ui) = weak.upgrade() {
                ui.probed(id, result);
            }
        });
    }

    /// Why a request may no longer proceed, or None while it is still exactly what the user chose.
    fn open_invalid(&self, pending: &opening::Pending) -> Option<String> {
        let quiet_only = self.model.borrow().pending.is_none() || self.pending_is_quiet();
        if self.ops.running.borrow().is_some() || self.ops.dialog.borrow().is_some() || self.ops.paste_reading.get() || !quiet_only {
            return Some("Another action started, so nothing was opened.".into());
        }
        if self.open_scope() != pending.scope {
            return Some("The folder, tab or search changed, so nothing was opened.".into());
        }
        let name = leaf(&pending.path);
        if pending.scope.search.is_some() {
            if self.opener.search_selected.borrow().as_ref().map(|entry| entry.path.as_path()) != Some(pending.path.as_path()) {
                return Some("The selection changed, so nothing was opened.".into());
            }
        } else {
            if self.ops.active.borrow().as_deref() != Some(pending.path.as_path()) {
                return Some("The selection changed, so nothing was opened.".into());
            }
            if !self.model.borrow().columns.iter().any(|column| column.entries.iter().any(|entry| entry.path == pending.path)) {
                return Some(format!("“{name}” is no longer in this folder, so nothing was opened."));
            }
        }
        None
    }

    /// End the request without an error (refused, superseded or cancelled).
    fn open_end(&self, id: u64, status: &'static str, message: String) {
        if !self.opener.tracker.borrow_mut().finish(id, status, message.clone()) {
            return;
        }
        self.ops.notice.replace(Some(message.clone()));
        self.status.set_text(&message);
        self.update_operation_controls();
        self.dump();
    }

    /// End the request with an error that stays visible until the next action.
    fn open_failed(&self, id: u64, message: String) {
        if !self.opener.tracker.borrow_mut().finish(id, "failed", message.clone()) {
            return;
        }
        self.model.borrow_mut().error = Some(message.clone());
        self.error.set_text(&message);
        self.error.set_visible(true);
        self.status.set_text("Nothing was opened.");
        self.update_operation_controls();
        self.dump();
    }

    fn probed(self: &Rc<Self>, id: u64, result: Result<Probe, glib::Error>) {
        let Some(pending) = self.opener.tracker.borrow().current(id).cloned() else {
            return;
        };
        if let Some(reason) = self.open_invalid(&pending) {
            self.open_end(id, "cancelled", reason);
            return;
        }
        let name = leaf(&pending.path);
        let probe = match result {
            Ok(probe) => probe,
            Err(error) => {
                self.open_failed(id, format!("Could not open “{name}”: {}", error.message()));
                return;
            }
        };
        if let Err(refusal) = opening::judge(&pending.path, &probe, pending.purpose) {
            self.open_end(id, refusal.tag(), refusal.message(&name));
            return;
        }
        match pending.purpose {
            Purpose::Default => self.open_default(&pending, probe.content_type),
            Purpose::Chooser => self.open_chooser(&pending, probe.content_type),
        }
    }

    fn open_default(self: &Rc<Self>, pending: &opening::Pending, content_type: String) {
        self.opener.tracker.borrow_mut().advance(pending.id, Stage::Launching);
        let (weak, id, lookup) = (Rc::downgrade(self), pending.id, content_type.clone());
        opening::default_app(&content_type, &pending.cancel, move |result| {
            if let Some(ui) = weak.upgrade() {
                ui.default_found(id, &lookup, result);
            }
        });
    }

    fn default_found(self: &Rc<Self>, id: u64, content_type: &str, result: Result<gio::AppInfo, glib::Error>) {
        let Some(pending) = self.opener.tracker.borrow().current(id).cloned() else {
            return;
        };
        match result {
            Ok(app) => self.launch_app(pending, app),
            Err(error) if error.matches(gio::IOErrorEnum::Cancelled) => {}
            Err(error) => {
                let kind = if content_type.is_empty() { "this type of".to_string() } else { gio::content_type_get_description(content_type).to_string() };
                self.open_failed(
                    id,
                    format!("No default application is registered for {kind} files, so “{}” was not opened ({}). Use Open With… to choose one.", leaf(&pending.path), error.message()),
                );
            }
        }
    }

    fn launch_app(self: &Rc<Self>, pending: opening::Pending, app: gio::AppInfo) {
        if let Some(reason) = self.open_invalid(&pending) {
            self.open_end(pending.id, "cancelled", reason);
            return;
        }
        self.opener.tracker.borrow_mut().advance(pending.id, Stage::Launching);
        let (name, app_name) = (leaf(&pending.path), app.name().to_string());
        self.status.set_text(&format!("Opening “{name}” with {app_name}…"));
        let context = gtk::prelude::WidgetExt::display(&self.window).app_launch_context();
        let (weak, id) = (Rc::downgrade(self), pending.id);
        opening::launch_with(&app, &pending.path, Some(&context), &pending.cancel, move |result| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            match result {
                Ok(()) => ui.open_end(id, "launched", format!("Opened “{name}” with {app_name}.")),
                Err(error) if error.matches(gio::IOErrorEnum::Cancelled) => {}
                Err(error) => ui.open_failed(id, format!("Could not open “{name}” with {app_name}: {}", error.message())),
            }
        });
    }

    /// The native application chooser. Cancel, Escape and close are inert.
    fn open_chooser(self: &Rc<Self>, pending: &opening::Pending, content_type: String) {
        self.opener.tracker.borrow_mut().advance(pending.id, Stage::Choosing);
        let name = leaf(&pending.path);
        let dialog = gtk::Dialog::builder().transient_for(&self.window).modal(true).destroy_with_parent(true).title("Open With").default_width(480).default_height(540).build();
        dialog.add_button("Cancel", gtk::ResponseType::Cancel);
        dialog.add_button("Open", gtk::ResponseType::Accept);
        dialog.set_default_response(gtk::ResponseType::Accept);
        let body = dialog.content_area();
        body.set_spacing(10);
        body.set_margin_top(18);
        body.set_margin_bottom(18);
        body.set_margin_start(18);
        body.set_margin_end(18);
        let label = gtk::Label::new(Some(&format!("Choose an application to open “{name}”.\nThis does not change the default application.")));
        label.set_wrap(true);
        label.set_xalign(0.0);
        body.append(&label);
        let chooser = gtk::AppChooserWidget::new(if content_type.is_empty() { "application/octet-stream" } else { content_type.as_str() });
        chooser.set_show_default(true);
        chooser.set_show_recommended(true);
        chooser.set_show_fallback(true);
        chooser.set_show_other(true);
        chooser.set_vexpand(true);
        chooser.update_property(&[gtk::accessible::Property::Label("Applications")]);
        body.append(&chooser);
        let accept = dialog.widget_for_response(gtk::ResponseType::Accept).unwrap();
        accept.set_sensitive(chooser.app_info().is_some());
        let enable = accept.clone();
        chooser.connect_application_selected(move |_, _| enable.set_sensitive(true));
        let activated = dialog.clone();
        chooser.connect_application_activated(move |_, _| activated.response(gtk::ResponseType::Accept));
        self.ops.dialog.replace(Some(dialog.clone()));
        self.update_operation_controls();
        self.dump();
        let (weak, id) = (Rc::downgrade(self), pending.id);
        dialog.connect_response(move |dialog, response| {
            let app = (response == gtk::ResponseType::Accept).then(|| chooser.app_info()).flatten();
            let Some(ui) = weak.upgrade() else {
                dialog.destroy();
                return;
            };
            ui.ops.dialog.borrow_mut().take();
            dialog.destroy();
            ui.chosen(id, app);
        });
        dialog.present();
    }

    fn chosen(self: &Rc<Self>, id: u64, app: Option<gio::AppInfo>) {
        let Some(pending) = self.opener.tracker.borrow().current(id).cloned() else {
            return;
        };
        match app {
            Some(app) => self.launch_app(pending, app),
            None => self.open_end(id, "cancelled", "Open With was cancelled. Nothing was opened and no default application was changed.".into()),
        }
    }

    /// Context menu for the row under the pointer: select it through the view's own
    /// selection (so the model validates it), then offer Open and Open With on that item.
    pub fn row_menu(self: &Rc<Self>, slot: SelectionSlot, generation: u64) -> RowMenu {
        let weak = Rc::downgrade(self);
        Rc::new(move |position: u32, widget: &gtk::Widget, x: f64, y: f64| {
            let Some(ui) = weak.upgrade().filter(|ui| ui.render_generation.get() == generation) else {
                return;
            };
            let selection = slot.borrow().clone();
            if let Some(selection) = selection {
                if selection.selected() != position {
                    selection.set_selected(position);
                }
            }
            ui.show_open_menu(widget, Some((x, y)));
        })
    }

    /// Menu / Shift+F10 on a file view opens the same menu for the current selection.
    pub fn attach_open_keys(self: &Rc<Self>, view: &impl IsA<gtk::Widget>, generation: u64) {
        let keys = gtk::EventControllerKey::new();
        let anchor = AsRef::<gtk::Widget>::as_ref(view).downgrade();
        let weak = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, modifiers| {
            let menu = key == gtk::gdk::Key::Menu || (key == gtk::gdk::Key::F10 && modifiers.contains(gtk::gdk::ModifierType::SHIFT_MASK));
            let ui = weak.upgrade().filter(|ui| menu && ui.render_generation.get() == generation);
            match (ui, anchor.upgrade()) {
                (Some(ui), Some(anchor)) => {
                    ui.show_open_menu(&anchor, None);
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });
        view.add_controller(keys);
    }

    pub fn show_open_menu(self: &Rc<Self>, anchor: &gtk::Widget, at: Option<(f64, f64)>) {
        self.close_open_menu();
        self.update_open_controls();
        if self.open_target().is_none() {
            return;
        }
        let menu = gio::Menu::new();
        menu.append(Some("Open"), Some("fileopen.open"));
        menu.append(Some("Open With…"), Some("fileopen.open-with"));
        let popover = gtk::PopoverMenu::from_model(Some(&menu));
        popover.set_parent(anchor);
        popover.set_has_arrow(false);
        if let Some((x, y)) = at {
            popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        }
        popover.connect_closed(|popover| {
            let popover = popover.clone();
            glib::idle_add_local_once(move || popover.unparent());
        });
        self.opener.menu.replace(Some(popover.clone()));
        popover.popup();
    }

    /// Before views are replaced, so a popover is never left parented to a destroyed view.
    pub fn close_open_menu(&self) {
        let menu = self.opener.menu.borrow_mut().take();
        if let Some(menu) = menu {
            menu.popdown();
            menu.unparent();
        }
    }
}

#[cfg(not(feature = "fixture-delay"))]
fn probe(path: &Path, cancel: &gio::Cancellable, done: impl FnOnce(Result<Probe, glib::Error>) + 'static) {
    opening::probe(path, cancel, done);
}

/// Fixture builds only: a capped delay before the probe makes races observable.
#[cfg(feature = "fixture-delay")]
fn probe(path: &Path, cancel: &gio::Cancellable, done: impl FnOnce(Result<Probe, glib::Error>) + 'static) {
    match std::env::var("DECFILES_FIXTURE_OPEN_DELAY_MS").ok().and_then(|value| value.parse::<u64>().ok()).filter(|delay| *delay > 0) {
        Some(delay) => {
            let (path, cancel): (std::path::PathBuf, gio::Cancellable) = (path.to_path_buf(), cancel.clone());
            glib::timeout_add_local_once(std::time::Duration::from_millis(delay.min(10_000)), move || opening::probe(&path, &cancel, done));
        }
        None => opening::probe(path, cancel, done),
    }
}
