mod operation_ui;
mod transfer_ui;
use decfiles::{
    loader,
    model::{self, Entry, Intent, Model},
    monitor::Monitor,
    resolver::Resolver,
};
use gio::prelude::*;
use gtk::prelude::*;
use std::{
    cell::{Cell, RefCell},
    io::Read,
    os::fd::AsRawFd,
    path::{Path, PathBuf},
    rc::Rc,
};

struct Ui {
    window: gtk::ApplicationWindow,
    model: RefCell<Model>,
    cancel: RefCell<Option<gio::Cancellable>>,
    address: gtk::Entry,
    content: gtk::Box,
    status: gtk::Label,
    error: gtk::Label,
    back: gtk::Button,
    forward: gtk::Button,
    up: gtk::Button,
    layout: gtk::DropDown,
    columns: Cell<bool>,
    /// Details sort (column id, descending). Survives re-render, view switches and navigation.
    sort: RefCell<(String, bool)>,
    view_model: RefCell<Option<gtk::SingleSelection>>,
    /// Incremented per render; selection signals from older views are ignored.
    render_generation: Cell<u64>,
    home: PathBuf,
    state_file: Option<PathBuf>,
    resolver: Resolver,
    resolver_source: RefCell<Option<glib::SourceId>>,
    monitor: Monitor,
    monitor_source: RefCell<Option<glib::SourceId>>,
    /// Watched folder and its watch revision; changes for other revisions are ignored.
    watched: RefCell<(u64, Option<PathBuf>)>,
    refresh_timer: RefCell<Option<glib::SourceId>>,
    quiet_revision: Cell<Option<u64>>,
    /// (background refreshes, refreshes whose visible entries were unchanged).
    monitor_stats: Cell<(u64, u64)>,
    ops: operation_ui::Controls,
}

impl Ui {
    fn navigate(self: &Rc<Self>, path: PathBuf, intent: Intent) {
        if !self.navigation_allowed() {
            return;
        }
        if let Some(old) = self.cancel.borrow_mut().take() {
            old.cancel();
        }
        let request = self.model.borrow_mut().request(path, intent);
        self.status.set_text("Resolving folder…");
        self.error.set_visible(false);
        self.dump();
        self.resolver.request(request);
        self.update_operation_controls();
    }

    fn resolved(self: &Rc<Self>, mut resolved: decfiles::resolver::Resolved) {
        if self.model.borrow().pending.as_ref().map(|request| request.revision) != Some(resolved.request.revision) {
            return;
        }
        let path = match resolved.result {
            Ok(path) => path,
            Err(error) => {
                self.model.borrow_mut().complete(&resolved.request, Err(error));
                self.render();
                return;
            }
        };
        resolved.request.path = path.clone();
        self.model.borrow_mut().pending = Some(resolved.request.clone());
        let cancel = gio::Cancellable::new();
        self.cancel.replace(Some(cancel.clone()));
        let quiet = self.quiet_revision.get() == Some(resolved.request.revision);
        if !quiet {
            self.status.set_text("Loading folder…");
        }
        self.dump();
        let weak = Rc::downgrade(self);
        loader::load(&path, &cancel, move |result| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let previous = quiet.then(|| ui.model.borrow().columns.last().map(|column| column.entries.clone())).flatten();
            let changed = ui.model.borrow_mut().complete(&resolved.request, result);
            if changed {
                ui.cancel.borrow_mut().take();
                ui.quiet_revision.set(None);
                ui.operation_refreshed();
                let model = ui.model.borrow();
                let unchanged = model.error.is_none() && previous.is_some() && previous.as_ref() == model.columns.last().map(|column| &column.entries);
                drop(model);
                if unchanged {
                    // A background refresh found nothing visible to change: keep widgets, focus and scroll.
                    let (refreshes, same) = ui.monitor_stats.get();
                    ui.monitor_stats.set((refreshes, same + 1));
                    ui.update_operation_controls();
                    ui.dump();
                } else {
                    ui.render();
                }
                ui.watch_current();
            }
        });
    }

    /// Watch the committed current folder; one watch per window, replaced on navigation.
    fn watch_current(&self) {
        let current = self.model.borrow().current().map(Path::to_path_buf);
        let mut watched = self.watched.borrow_mut();
        if current.is_none() || watched.1 == current {
            return;
        }
        watched.0 += 1;
        watched.1 = current.clone();
        self.monitor.watch(watched.0, current.unwrap());
        drop(watched);
        self.dump();
    }

    fn folder_changed(self: &Rc<Self>) {
        if self.refresh_timer.borrow().is_some() {
            return;
        }
        // Debounce bursts; while busy (dialog, operation, navigation) wait and retry.
        let weak = Rc::downgrade(self);
        let timer = glib::timeout_add_local(std::time::Duration::from_millis(300), move || {
            let Some(ui) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let idle = !ui.ops.is_busy() && ui.ops.dialog.borrow().is_none() && ui.model.borrow().pending.is_none() && !ui.ops.paste_reading.get();
            if !idle {
                return glib::ControlFlow::Continue;
            }
            ui.refresh_timer.borrow_mut().take();
            ui.refresh_quietly();
            glib::ControlFlow::Break
        });
        self.refresh_timer.replace(Some(timer));
    }

    fn refresh_quietly(self: &Rc<Self>) {
        let Some(path) = self.model.borrow().current().map(Path::to_path_buf) else {
            return;
        };
        let request = self.model.borrow_mut().request(path, Intent::Refresh);
        self.quiet_revision.set(Some(request.revision));
        let (refreshes, same) = self.monitor_stats.get();
        self.monitor_stats.set((refreshes + 1, same));
        self.resolver.request(request);
        self.update_operation_controls();
        self.dump();
    }

    fn history(self: &Rc<Self>, intent: Intent) {
        let path = self.model.borrow().history_target(&intent);
        if let Some(path) = path {
            self.navigate(path, intent);
        }
    }

    fn address_go(self: &Rc<Self>) {
        let model = self.model.borrow();
        let base = model.current().unwrap_or(&self.home);
        let result = model::location(self.address.text().as_str(), base, &self.home);
        drop(model);
        match result {
            Ok(path) => self.navigate(path, Intent::Visit),
            Err(error) => {
                if let Some(cancel) = self.cancel.borrow_mut().take() {
                    cancel.cancel();
                }
                let mut model = self.model.borrow_mut();
                model.pending = None;
                model.error = Some(error);
                drop(model);
                self.render();
            }
        }
    }

    fn selected(&self, column: usize, selection: &gtk::SingleSelection) {
        let path = selection.selected_item().and_downcast::<glib::BoxedAnyObject>().map(|object| object.borrow::<Entry>().path.clone());
        self.model.borrow_mut().select(column, path);
        // Operations act only on a selection the committed model has validated.
        let path = self.model.borrow().columns.get(column).and_then(|column| column.selected.clone());
        self.ops.active.replace(path.clone());
        if !self.ops.is_busy() {
            self.ops.clear_notice();
            if let Some(path) = path {
                self.status.set_text(&format!("Selected: {}", path.file_name().unwrap_or_default().to_string_lossy()));
            }
        }
        self.update_operation_controls();
        self.dump();
    }

    fn activate(self: &Rc<Self>, selection: &gtk::SingleSelection, position: u32) {
        let Some(object) = selection.item(position).and_downcast::<glib::BoxedAnyObject>() else {
            return;
        };
        let entry = object.borrow::<Entry>();
        if entry.directory {
            self.navigate(entry.path.clone(), Intent::Visit);
        } else {
            self.status.set_text("File selected. Opening files is not implemented in this prototype.");
        }
    }

    fn render(self: &Rc<Self>) {
        self.reconcile_operation_selection();
        // Keep keyboard focus in the file list across re-renders (refreshes, sorting, monitoring).
        let had_focus = gtk::prelude::RootExt::focus(&self.window).is_some_and(|focus| focus.is_ancestor(&self.content));
        let mut last_view: Option<gtk::Widget> = None;
        while let Some(child) = self.content.first_child() {
            self.content.remove(&child);
        }
        let model = self.model.borrow();
        self.back.set_sensitive(model.can_back());
        self.forward.set_sensitive(model.can_forward());
        self.up.set_sensitive(model.current().and_then(Path::parent).is_some());
        if let Some(path) = model.current() {
            self.address.set_text(&path.to_string_lossy());
        }
        self.error.set_text(model.error.as_deref().unwrap_or(""));
        self.error.set_visible(model.error.is_some());
        self.status.set_text(&format!("{} items", model.columns.last().map_or(0, |column| column.entries.len())));
        let shown: Vec<_> = if self.columns.get() { model.columns.iter().enumerate().collect() } else { model.columns.iter().enumerate().next_back().into_iter().collect() };
        self.view_model.replace(None);
        let generation = self.render_generation.get().wrapping_add(1);
        self.render_generation.set(generation);
        for (index, column) in shown {
            let store = gio::ListStore::new::<glib::BoxedAnyObject>();
            for entry in &column.entries {
                store.append(&glib::BoxedAnyObject::new(entry.clone()));
            }
            let scroll = gtk::ScrolledWindow::builder().vexpand(true).hexpand(!self.columns.get()).min_content_width(if self.columns.get() { 250 } else { 400 }).build();
            let details = (!self.columns.get()).then(|| self.details_view());
            // Details sort natively; folders always stay first, as in the model order.
            let items: gio::ListModel = match &details {
                Some(view) => {
                    let sorter = gtk::MultiSorter::new();
                    sorter.append(folders_first());
                    if let Some(chosen) = view.sorter() {
                        sorter.append(chosen);
                    }
                    gtk::SortListModel::new(Some(store), Some(sorter)).upcast()
                }
                None => store.upcast(),
            };
            let selection = gtk::SingleSelection::new(Some(items));
            selection.set_autoselect(false);
            selection.set_can_unselect(true);
            let position = column.selected.as_ref().and_then(|path| (0..selection.n_items()).find(|&i| entry_at(&selection, i).is_some_and(|entry| &entry.path == path)));
            selection.set_selected(position.unwrap_or(gtk::INVALID_LIST_POSITION));
            let weak = Rc::downgrade(self);
            selection.connect_selected_notify(move |selection| {
                // A disposed view can still notify (its sorter re-sorts while torn down).
                if let Some(ui) = weak.upgrade().filter(|ui| ui.render_generation.get() == generation) {
                    ui.selected(index, selection);
                }
            });
            match details {
                None => {
                    let list = gtk::ListView::new(Some(selection.clone()), Some(factory(0)));
                    list.set_single_click_activate(true);
                    list.update_property(&[gtk::accessible::Property::Label(&format!("Folder column {}", column.path.display()))]);
                    let selected = selection.clone();
                    let weak = Rc::downgrade(self);
                    list.connect_activate(move |_, position| {
                        if let Some(ui) = weak.upgrade() {
                            ui.activate(&selected, position);
                        }
                    });
                    scroll.set_child(Some(&list));
                    last_view = Some(list.clone().upcast());
                    let box_ = gtk::Box::new(gtk::Orientation::Vertical, 0);
                    let title = gtk::Label::new(Some(&column.path.file_name().unwrap_or_else(|| column.path.as_os_str()).to_string_lossy()));
                    title.add_css_class("heading");
                    title.set_margin_top(8);
                    title.set_margin_bottom(8);
                    box_.append(&title);
                    box_.append(&scroll);
                    self.content.append(&box_);
                }
                Some(view) => {
                    view.set_model(Some(&selection));
                    let selected = selection.clone();
                    let weak = Rc::downgrade(self);
                    view.connect_activate(move |_, position| {
                        if let Some(ui) = weak.upgrade() {
                            ui.activate(&selected, position);
                        }
                    });
                    self.view_model.replace(Some(selection.clone()));
                    scroll.set_child(Some(&view));
                    last_view = Some(view.clone().upcast());
                    self.content.append(&scroll);
                }
            }
        }
        drop(model);
        if let Some(view) = last_view.filter(|_| had_focus) {
            view.grab_focus();
        }
        if let Some(notice) = self.ops.notice() {
            self.status.set_text(&notice);
        }
        self.update_operation_controls();
        self.dump();
    }

    /// Name, Type, Size and Modified with native header sorting. The saved choice is
    /// re-applied on every render and updated when the user clicks a header.
    fn details_view(self: &Rc<Self>) -> gtk::ColumnView {
        let view = gtk::ColumnView::new(None::<gtk::SingleSelection>);
        view.update_property(&[gtk::accessible::Property::Label("Folder details")]);
        let (sorted_by, descending) = self.sort.borrow().clone();
        for (id, title, kind) in [("name", "Name", 0u8), ("type", "Type", 1), ("size", "Size", 2), ("modified", "Date modified", 3)] {
            let column = gtk::ColumnViewColumn::new(Some(title), Some(factory(kind)));
            column.set_id(Some(id));
            column.set_expand(kind == 0);
            column.set_resizable(true);
            column.set_sorter(Some(&entry_sorter(kind)));
            view.append_column(&column);
            if id == sorted_by {
                view.sort_by_column(Some(&column), if descending { gtk::SortType::Descending } else { gtk::SortType::Ascending });
            }
        }
        let weak = Rc::downgrade(self);
        if let Some(sorter) = view.sorter().and_downcast::<gtk::ColumnViewSorter>() {
            sorter.connect_changed(move |sorter, _| {
                let Some(ui) = weak.upgrade() else {
                    return;
                };
                // A disposed view reports no primary column; header clicks only toggle order.
                let Some(id) = sorter.primary_sort_column().and_then(|column| column.id()) else {
                    return;
                };
                ui.sort.replace((id.to_string(), sorter.primary_sort_order() == gtk::SortType::Descending));
                ui.dump();
            });
        }
        view
    }

    fn dump(&self) {
        // Explicit opt-in fixture hook. Never enabled by default or written into shell config.
        if let Some(path) = &self.state_file {
            let model = self.model.borrow();
            let (sorted_by, descending) = self.sort.borrow().clone();
            // Visible details order (bounded) proves native sorting, not just model order.
            let view_order: Vec<String> = self
                .view_model
                .borrow()
                .as_ref()
                .map_or_else(Vec::new, |selection| (0..selection.n_items().min(256)).filter_map(|i| entry_at(selection, i).map(|entry| entry.path.to_string_lossy().into_owned())).collect());
            let value = serde_json::json!({"layout":if self.columns.get() {"columns"} else {"details"}, "model":model.evidence(), "operation":self.ops.evidence(), "status":self.status.text().as_str(),
                "sort":{"column":sorted_by,"descending":descending}, "view_order":view_order,
                "monitor":{"watching":self.watched.borrow().1.as_ref().map(|path| path.to_string_lossy().into_owned()),"refreshes":self.monitor_stats.get().0,"unchanged":self.monitor_stats.get().1}});
            if let Ok(bytes) = serde_json::to_vec_pretty(&value) {
                let staging = path.with_extension("tmp");
                if std::fs::write(&staging, bytes).is_ok() {
                    let _ = std::fs::rename(staging, path);
                }
            }
        }
    }
}

fn entry_at(model: &impl IsA<gio::ListModel>, position: u32) -> Option<Entry> {
    model.item(position).and_downcast::<glib::BoxedAnyObject>().map(|object| object.borrow::<Entry>().clone())
}

fn compare(a: &glib::Object, b: &glib::Object, by: impl Fn(&Entry, &Entry) -> std::cmp::Ordering) -> gtk::Ordering {
    let (Some(a), Some(b)) = (a.downcast_ref::<glib::BoxedAnyObject>(), b.downcast_ref::<glib::BoxedAnyObject>()) else {
        return gtk::Ordering::Equal;
    };
    by(&a.borrow::<Entry>(), &b.borrow::<Entry>()).into()
}

fn folders_first() -> gtk::CustomSorter {
    gtk::CustomSorter::new(|a, b| compare(a, b, |a, b| b.directory.cmp(&a.directory)))
}

/// Ties fall back to the case-insensitive name and then exact path bytes, so order is stable.
fn entry_sorter(kind: u8) -> gtk::CustomSorter {
    gtk::CustomSorter::new(move |a, b| {
        compare(a, b, |a, b| {
            let primary = match kind {
                1 => kind_text(a).to_lowercase().cmp(&kind_text(b).to_lowercase()),
                2 => a.size.cmp(&b.size),
                3 => a.modified.cmp(&b.modified),
                _ => std::cmp::Ordering::Equal,
            };
            primary.then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())).then_with(|| a.path.cmp(&b.path))
        })
    })
}

fn kind_text(entry: &Entry) -> String {
    if entry.directory {
        "Folder".into()
    } else if entry.content_type.is_empty() {
        "File".into()
    } else {
        gio::content_type_get_description(&entry.content_type).to_string()
    }
}

fn entry_icon(entry: &Entry) -> gio::Icon {
    if entry.directory {
        gio::ThemedIcon::new("folder").upcast()
    } else if entry.content_type.is_empty() {
        gio::ThemedIcon::new("text-x-generic").upcast()
    } else {
        gio::content_type_get_icon(&entry.content_type)
    }
}

fn factory(kind: u8) -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(move |_, object| {
        let item = object.downcast_ref::<gtk::ListItem>().unwrap();
        let cell = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        cell.set_margin_top(8);
        cell.set_margin_bottom(8);
        cell.set_margin_start(12);
        cell.set_margin_end(12);
        if kind == 0 {
            // Icons come from the user's GTK icon theme for the GIO content type.
            let icon = gtk::Image::new();
            icon.set_pixel_size(16);
            icon.set_accessible_role(gtk::AccessibleRole::Presentation);
            cell.append(&icon);
        }
        let label = gtk::Label::new(None);
        label.set_xalign(if kind == 2 { 1.0 } else { 0.0 });
        label.set_hexpand(true);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        cell.append(&label);
        item.set_child(Some(&cell));
    });
    factory.connect_bind(move |_, object| {
        let item = object.downcast_ref::<gtk::ListItem>().unwrap();
        let Some(object) = item.item().and_downcast::<glib::BoxedAnyObject>() else {
            return;
        };
        let entry = object.borrow::<Entry>();
        let cell = item.child().and_downcast::<gtk::Box>().unwrap();
        let value = match kind {
            1 => kind_text(&entry),
            2 if entry.directory => String::new(),
            2 => glib::format_size(entry.size).to_string(),
            3 => glib::DateTime::from_unix_local(entry.modified as i64).and_then(|time| time.format("%Y-%m-%d %H:%M")).map_or_else(|_| String::new(), |text| text.to_string()),
            _ if entry.directory => format!("{}  ›", entry.name),
            _ => entry.name.clone(),
        };
        if kind == 0 {
            cell.first_child().and_downcast::<gtk::Image>().unwrap().set_from_gicon(&entry_icon(&entry));
        }
        cell.last_child().and_downcast::<gtk::Label>().unwrap().set_text(&value);
    });
    factory
}

fn build(app: &gtk::Application, initial: PathBuf, columns: bool) {
    let window = gtk::ApplicationWindow::builder().application(app).title("Decebal Files — Preview").default_width(1080).default_height(700).build();
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    toolbar.set_margin_top(10);
    toolbar.set_margin_bottom(10);
    toolbar.set_margin_start(12);
    toolbar.set_margin_end(12);
    let back = gtk::Button::with_label("Back");
    let forward = gtk::Button::with_label("Forward");
    let up = gtk::Button::with_label("Up");
    for button in [&back, &forward, &up] {
        toolbar.append(button);
    }
    let address = gtk::Entry::builder().hexpand(true).placeholder_text("Folder location").build();
    address.update_property(&[gtk::accessible::Property::Label("Folder location")]);
    toolbar.append(&address);
    let go = gtk::Button::with_label("Go");
    toolbar.append(&go);
    let layout = gtk::DropDown::from_strings(&["Windows details", "macOS columns"]);
    layout.set_selected(u32::from(columns));
    layout.update_property(&[gtk::accessible::Property::Label("Browser layout")]);
    toolbar.append(&layout);
    root.append(&toolbar);
    let error = gtk::Label::new(None);
    error.set_wrap(true);
    error.set_xalign(0.0);
    error.set_margin_start(12);
    error.set_margin_end(12);
    error.add_css_class("error");
    root.append(&error);
    let body = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    body.set_vexpand(true);
    let sidebar = gtk::Box::new(gtk::Orientation::Vertical, 4);
    sidebar.set_width_request(170);
    sidebar.set_margin_start(8);
    sidebar.set_margin_end(8);
    sidebar.set_margin_top(8);
    let sidebar_title = gtk::Label::new(Some("Locations"));
    sidebar_title.add_css_class("heading");
    sidebar.append(&sidebar_title);
    body.append(&sidebar);
    body.append(&gtk::Separator::new(gtk::Orientation::Vertical));
    let horizontal = gtk::ScrolledWindow::builder().hexpand(true).vexpand(true).hscrollbar_policy(gtk::PolicyType::Automatic).vscrollbar_policy(gtk::PolicyType::Never).build();
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 1);
    horizontal.set_child(Some(&content));
    body.append(&horizontal);
    root.append(&body);
    let ops = operation_ui::Controls::new();
    root.append(&ops.progress_row());
    root.append(&ops.bar());
    let status = gtk::Label::new(Some("Files preview"));
    status.set_xalign(0.0);
    // Long transfer summaries must not widen the window; failures also use the wrapping error label.
    status.set_ellipsize(gtk::pango::EllipsizeMode::End);
    status.set_margin_start(12);
    status.set_margin_top(8);
    status.set_margin_bottom(8);
    root.append(&status);
    window.set_child(Some(&root));
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"));
    let (resolver, mut resolver_reader) = Resolver::new().expect("Could not create Files path resolver");
    let (monitor, mut monitor_reader) = Monitor::new().expect("Could not create Files folder monitor");
    let ui = Rc::new(Ui {
        window,
        model: RefCell::new(Model::default()),
        cancel: RefCell::new(None),
        address,
        content,
        status,
        error,
        back,
        forward,
        up,
        layout,
        columns: Cell::new(columns),
        sort: RefCell::new(("name".into(), false)),
        view_model: RefCell::new(None),
        render_generation: Cell::new(0),
        home: home.clone(),
        state_file: std::env::var_os("DECFILES_EVIDENCE_STATE").map(PathBuf::from),
        resolver,
        resolver_source: RefCell::new(None),
        monitor,
        monitor_source: RefCell::new(None),
        watched: RefCell::new((0, None)),
        refresh_timer: RefCell::new(None),
        quiet_revision: Cell::new(None),
        monitor_stats: Cell::new((0, 0)),
        ops,
    });
    let weak = Rc::downgrade(&ui);
    let source = glib::source::unix_fd_add_local(resolver_reader.as_raw_fd(), glib::IOCondition::IN | glib::IOCondition::HUP, move |_, condition| {
        let Some(ui) = weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        let mut bytes = [0; 128];
        while matches!(resolver_reader.read(&mut bytes), Ok(n) if n > 0) {}
        if let Some(result) = ui.resolver.take_result() {
            ui.resolved(result);
        }
        if condition.contains(glib::IOCondition::HUP) {
            glib::ControlFlow::Break
        } else {
            glib::ControlFlow::Continue
        }
    });
    ui.resolver_source.replace(Some(source));
    let weak = Rc::downgrade(&ui);
    let source = glib::source::unix_fd_add_local(monitor_reader.as_raw_fd(), glib::IOCondition::IN | glib::IOCondition::HUP, move |_, condition| {
        let Some(ui) = weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        let mut bytes = [0; 128];
        while matches!(monitor_reader.read(&mut bytes), Ok(n) if n > 0) {}
        if ui.monitor.take_change().is_some_and(|revision| revision == ui.watched.borrow().0) {
            ui.folder_changed();
        }
        if condition.contains(glib::IOCondition::HUP) {
            glib::ControlFlow::Break
        } else {
            glib::ControlFlow::Continue
        }
    });
    ui.monitor_source.replace(Some(source));
    for (label, path) in [("Home", home.clone()), ("Documents", home.join("Documents")), ("Downloads", home.join("Downloads")), ("File system", PathBuf::from("/"))] {
        let button = gtk::Button::with_label(label);
        sidebar.append(&button);
        let weak = Rc::downgrade(&ui);
        button.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.navigate(path.clone(), Intent::Visit);
            }
        });
    }
    let weak = Rc::downgrade(&ui);
    ui.back.connect_clicked(move |_| {
        if let Some(ui) = weak.upgrade() {
            ui.history(Intent::Back);
        }
    });
    let weak = Rc::downgrade(&ui);
    ui.forward.connect_clicked(move |_| {
        if let Some(ui) = weak.upgrade() {
            ui.history(Intent::Forward);
        }
    });
    let weak = Rc::downgrade(&ui);
    ui.up.connect_clicked(move |_| {
        if let Some(ui) = weak.upgrade() {
            let parent = ui.model.borrow().current().and_then(Path::parent).map(Path::to_path_buf);
            if let Some(path) = parent {
                ui.navigate(path, Intent::Visit);
            }
        }
    });
    let weak = Rc::downgrade(&ui);
    ui.address.connect_activate(move |_| {
        if let Some(ui) = weak.upgrade() {
            ui.address_go();
        }
    });
    let weak = Rc::downgrade(&ui);
    go.connect_clicked(move |_| {
        if let Some(ui) = weak.upgrade() {
            ui.address_go();
        }
    });
    let weak = Rc::downgrade(&ui);
    ui.layout.connect_selected_notify(move |layout| {
        if let Some(ui) = weak.upgrade() {
            ui.columns.set(layout.selected() == 1);
            ui.render();
        }
    });
    ui.connect_operations();
    ui.connect_transfers();
    let controller = gtk::EventControllerKey::new();
    controller.set_propagation_phase(gtk::PropagationPhase::Capture);
    let weak = Rc::downgrade(&ui);
    controller.connect_key_pressed(move |_, key, _, modifiers| {
        let Some(ui) = weak.upgrade() else {
            return glib::Propagation::Proceed;
        };
        let alt = modifiers.contains(gtk::gdk::ModifierType::ALT_MASK);
        let ctrl = modifiers.contains(gtk::gdk::ModifierType::CONTROL_MASK);
        if ctrl && modifiers.contains(gtk::gdk::ModifierType::SHIFT_MASK) && (key == gtk::gdk::Key::n || key == gtk::gdk::Key::N) {
            ui.name_dialog(false);
        } else if key == gtk::gdk::Key::F2 {
            ui.name_dialog(true);
        } else if key == gtk::gdk::Key::Delete
            && !ctrl
            && !alt
            && !modifiers.contains(gtk::gdk::ModifierType::SHIFT_MASK)
            && gtk::prelude::RootExt::focus(&ui.window).is_some_and(|focus| focus.is_ancestor(&ui.content))
        {
            ui.trash_dialog();
        } else if ctrl
            && !alt
            && [gtk::gdk::Key::c, gtk::gdk::Key::x, gtk::gdk::Key::v].contains(&key.to_lower())
            && gtk::prelude::RootExt::focus(&ui.window).is_some_and(|focus| focus.is_ancestor(&ui.content))
        {
            // Only in the file list; the address entry keeps ordinary text clipboard keys.
            match key.to_lower() {
                gtk::gdk::Key::c => ui.clipboard_put(decfiles::transfer::Mode::Copy),
                gtk::gdk::Key::x => ui.clipboard_put(decfiles::transfer::Mode::Move),
                _ => ui.paste(),
            }
        } else if ctrl && (key == gtk::gdk::Key::w || key == gtk::gdk::Key::W) {
            ui.window.close();
        } else if ctrl && (key == gtk::gdk::Key::l || key == gtk::gdk::Key::L) {
            ui.address.grab_focus();
            ui.address.select_region(0, -1);
        } else if alt && key == gtk::gdk::Key::Left {
            ui.history(Intent::Back);
        } else if alt && key == gtk::gdk::Key::Right {
            ui.history(Intent::Forward);
        } else if alt && key == gtk::gdk::Key::Up {
            ui.up.emit_clicked();
        } else if ctrl && (key == gtk::gdk::Key::r || key == gtk::gdk::Key::R) {
            let path = ui.model.borrow().current().map(Path::to_path_buf);
            if let Some(path) = path {
                ui.navigate(path, Intent::Refresh);
            }
        } else {
            return glib::Propagation::Proceed;
        }
        glib::Propagation::Stop
    });
    ui.window.add_controller(controller);
    // The window owns one UI reference until destroy; all child signal handlers are weak.
    let retained = ui.clone();
    ui.window.connect_destroy(move |_| {
        retained.resolver.stop();
        retained.monitor.stop();
        for source in [retained.monitor_source.borrow_mut().take(), retained.refresh_timer.borrow_mut().take()].into_iter().flatten() {
            source.remove();
        }
        if let Some(source) = retained.resolver_source.borrow_mut().take() {
            source.remove();
        }
        if let Some(cancel) = retained.cancel.borrow_mut().take() {
            cancel.cancel();
        }
    });
    ui.window.present();
    ui.navigate(initial, Intent::Visit);
}

fn main() {
    let mut initial = None;
    let mut columns = false;
    for arg in std::env::args().skip(1) {
        if arg == "--columns" {
            columns = true;
        } else if !arg.starts_with('-') {
            initial = Some(PathBuf::from(arg));
        }
    }
    let initial = initial.or_else(|| std::env::var_os("HOME").map(PathBuf::from)).unwrap_or_else(|| PathBuf::from("/"));
    let initial = if initial.is_absolute() { initial } else { std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")).join(initial) };
    let app = gtk::Application::builder().application_id("org.decebal.Files.Preview").flags(gio::ApplicationFlags::NON_UNIQUE).build();
    app.connect_activate(move |app| build(app, initial.clone(), columns));
    app.run_with_args::<&str>(&[]);
}
