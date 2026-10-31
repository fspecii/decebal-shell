use decfiles::{
    loader,
    model::{self, Entry, Intent, Model},
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
    home: PathBuf,
    state_file: Option<PathBuf>,
    resolver: Resolver,
    resolver_source: RefCell<Option<glib::SourceId>>,
}

impl Ui {
    fn navigate(self: &Rc<Self>, path: PathBuf, intent: Intent) {
        if let Some(old) = self.cancel.borrow_mut().take() {
            old.cancel();
        }
        let request = self.model.borrow_mut().request(path, intent);
        self.status.set_text("Resolving folder…");
        self.error.set_visible(false);
        self.dump();
        self.resolver.request(request);
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
        self.status.set_text("Loading folder…");
        self.dump();
        let weak = Rc::downgrade(self);
        loader::load(&path, &cancel, move |result| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let changed = ui.model.borrow_mut().complete(&resolved.request, result);
            if changed {
                ui.cancel.borrow_mut().take();
                ui.render();
            }
        });
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
        self.model.borrow_mut().select(column, path.clone());
        if let Some(path) = path {
            self.status.set_text(&format!("Selected: {} · Read-only prototype", path.file_name().unwrap_or_default().to_string_lossy()));
        }
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
            self.status.set_text("File selected. Opening files and file operations are not implemented in this read-only prototype.");
        }
    }

    fn render(self: &Rc<Self>) {
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
        self.status.set_text(&format!("{} items · Read-only prototype", model.columns.last().map_or(0, |column| column.entries.len())));
        let shown: Vec<_> = if self.columns.get() { model.columns.iter().enumerate().collect() } else { model.columns.iter().enumerate().next_back().into_iter().collect() };
        for (index, column) in shown {
            let store = gio::ListStore::new::<glib::BoxedAnyObject>();
            for entry in &column.entries {
                store.append(&glib::BoxedAnyObject::new(entry.clone()));
            }
            let selection = gtk::SingleSelection::new(Some(store));
            selection.set_autoselect(false);
            selection.set_can_unselect(true);
            selection.set_selected(column.selected.as_ref().and_then(|path| column.entries.iter().position(|entry| &entry.path == path)).map_or(gtk::INVALID_LIST_POSITION, |index| index as u32));
            let weak = Rc::downgrade(self);
            selection.connect_selected_notify(move |selection| {
                if let Some(ui) = weak.upgrade() {
                    ui.selected(index, selection);
                }
            });
            let scroll = gtk::ScrolledWindow::builder().vexpand(true).hexpand(!self.columns.get()).min_content_width(if self.columns.get() { 250 } else { 400 }).build();
            if self.columns.get() {
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
                let box_ = gtk::Box::new(gtk::Orientation::Vertical, 0);
                let title = gtk::Label::new(Some(&column.path.file_name().unwrap_or_else(|| column.path.as_os_str()).to_string_lossy()));
                title.add_css_class("heading");
                title.set_margin_top(8);
                title.set_margin_bottom(8);
                box_.append(&title);
                box_.append(&scroll);
                self.content.append(&box_);
            } else {
                let view = gtk::ColumnView::new(Some(selection.clone()));
                view.update_property(&[gtk::accessible::Property::Label("Folder details")]);
                for (name, kind) in [("Name", 0), ("Kind", 1), ("Size", 2)] {
                    let column = gtk::ColumnViewColumn::new(Some(name), Some(factory(kind)));
                    column.set_expand(kind == 0);
                    column.set_resizable(true);
                    view.append_column(&column);
                }
                let selected = selection.clone();
                let weak = Rc::downgrade(self);
                view.connect_activate(move |_, position| {
                    if let Some(ui) = weak.upgrade() {
                        ui.activate(&selected, position);
                    }
                });
                scroll.set_child(Some(&view));
                self.content.append(&scroll);
            }
        }
        drop(model);
        self.dump();
    }

    fn dump(&self) {
        // Explicit opt-in fixture hook. Never enabled by default or written into shell config.
        if let Some(path) = &self.state_file {
            let model = self.model.borrow();
            let value = serde_json::json!({"layout":if self.columns.get() {"columns"} else {"details"}, "model":model.evidence()});
            if let Ok(bytes) = serde_json::to_vec_pretty(&value) {
                let staging = path.with_extension("tmp");
                if std::fs::write(&staging, bytes).is_ok() {
                    let _ = std::fs::rename(staging, path);
                }
            }
        }
    }
}

fn factory(kind: u8) -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(move |_, object| {
        let item = object.downcast_ref::<gtk::ListItem>().unwrap();
        let label = gtk::Label::new(None);
        label.set_xalign(0.0);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.set_margin_top(8);
        label.set_margin_bottom(8);
        label.set_margin_start(12);
        label.set_margin_end(12);
        item.set_child(Some(&label));
    });
    factory.connect_bind(move |_, object| {
        let item = object.downcast_ref::<gtk::ListItem>().unwrap();
        let Some(object) = item.item().and_downcast::<glib::BoxedAnyObject>() else {
            return;
        };
        let entry = object.borrow::<Entry>();
        let value = match kind {
            1 => {
                if entry.directory {
                    "Folder".into()
                } else {
                    "File".into()
                }
            }
            2 => {
                if entry.directory {
                    "—".into()
                } else {
                    format!("{} B", entry.size)
                }
            }
            _ => {
                if entry.directory {
                    format!("{}  ›", entry.name)
                } else {
                    entry.name.clone()
                }
            }
        };
        item.child().and_downcast::<gtk::Label>().unwrap().set_text(&value);
    });
    factory
}

fn build(app: &gtk::Application, initial: PathBuf, columns: bool) {
    let window = gtk::ApplicationWindow::builder().application(app).title("Decebal Files — Read-only preview").default_width(1080).default_height(700).build();
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
    let status = gtk::Label::new(Some("Read-only prototype"));
    status.set_xalign(0.0);
    status.set_margin_start(12);
    status.set_margin_top(8);
    status.set_margin_bottom(8);
    root.append(&status);
    window.set_child(Some(&root));
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"));
    let (resolver, mut resolver_reader) = Resolver::new().expect("Could not create Files path resolver");
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
        home: home.clone(),
        state_file: std::env::var_os("DECFILES_EVIDENCE_STATE").map(PathBuf::from),
        resolver,
        resolver_source: RefCell::new(None),
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
    let controller = gtk::EventControllerKey::new();
    controller.set_propagation_phase(gtk::PropagationPhase::Capture);
    let weak = Rc::downgrade(&ui);
    controller.connect_key_pressed(move |_, key, _, modifiers| {
        let Some(ui) = weak.upgrade() else {
            return glib::Propagation::Proceed;
        };
        let alt = modifiers.contains(gtk::gdk::ModifierType::ALT_MASK);
        let ctrl = modifiers.contains(gtk::gdk::ModifierType::CONTROL_MASK);
        if ctrl && (key == gtk::gdk::Key::w || key == gtk::gdk::Key::W) {
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
