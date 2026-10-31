//! Search in the current folder: a native search entry starts a bounded worker; results
//! stream into a persistent list incrementally. A newer query, navigation, a tab switch
//! or Escape cancels the search; stale results are ignored by search id. Enter on a result
//! reveals it; opening a result is a separate explicit action (Open, Open With, context menu).
use super::Ui;
use decfiles::{
    model::{Entry, Intent},
    search::{Search, Status},
};
use gtk::prelude::*;
use std::{path::PathBuf, rc::Rc, time::Duration};

pub struct SearchRun {
    pub id: u64,
    search: Search,
    pub store: gio::ListStore,
    pub root: PathBuf,
    pub query: String,
    delivered: usize,
    pub status: Status,
}

impl Ui {
    pub fn connect_search(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.search_entry.connect_search_changed(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.search_changed();
            }
        });
        let weak = Rc::downgrade(self);
        self.search_entry.connect_stop_search(move |entry| {
            if let Some(ui) = weak.upgrade() {
                ui.end_search();
                entry.set_text("");
                ui.render();
            }
        });
    }

    pub fn is_searching(&self) -> bool {
        self.search.borrow().is_some()
    }

    fn search_changed(self: &Rc<Self>) {
        if self.search_clearing.get() {
            return;
        }
        let query = self.search_entry.text().to_string();
        self.end_search();
        if query.is_empty() {
            self.render();
            return;
        }
        self.drop_quiet_refresh();
        let Some(root) = self.model.borrow().current().map(std::path::Path::to_path_buf) else {
            return;
        };
        if self.ops.is_busy() || self.ops.dialog.borrow().is_some() || self.ops.paste_reading.get() {
            self.status.set_text("Finish the current operation before searching.");
            return;
        }
        let search = match start(root.clone(), &query) {
            Ok(search) => search,
            Err(error) => {
                self.status.set_text(&error.to_string());
                return;
            }
        };
        let id = self.search_seq.get() + 1;
        self.search_seq.set(id);
        let store = gio::ListStore::new::<glib::BoxedAnyObject>();
        self.search.replace(Some(SearchRun { id, search, store, root, query, delivered: 0, status: Status::default() }));
        self.render();
        let weak = Rc::downgrade(self);
        glib::timeout_add_local(Duration::from_millis(100), move || {
            let Some(ui) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            ui.poll_search(id)
        });
    }

    /// Append only new hits (incremental); a different id means this search was replaced.
    fn poll_search(&self, id: u64) -> glib::ControlFlow {
        let mut slot = self.search.borrow_mut();
        let Some(run) = slot.as_mut().filter(|run| run.id == id) else {
            return glib::ControlFlow::Break;
        };
        let (hits, status) = run.search.take_new(run.delivered);
        run.delivered += hits.len();
        for hit in hits {
            run.store.append(&glib::BoxedAnyObject::new(hit));
        }
        run.status = status.clone();
        drop(slot);
        self.search_status();
        self.dump();
        if status.done {
            glib::ControlFlow::Break
        } else {
            glib::ControlFlow::Continue
        }
    }

    fn search_status(&self) {
        let Some(run) = self.search.borrow().as_ref().map(|run| (run.query.clone(), run.store.n_items(), run.status.clone(), run.root.clone())) else {
            return;
        };
        let (query, count, status, root) = run;
        let folder = root.file_name().map_or_else(|| root.display().to_string(), |name| name.to_string_lossy().into_owned());
        let mut text = if status.done {
            format!("{count} results for “{query}” in “{folder}”.")
        } else {
            format!("Searching “{folder}” for “{query}”… {count} found, {} items checked.", status.visited)
        };
        if status.truncated {
            text.push_str(" Showing only the first results; refine the search to see more.");
        }
        if status.skipped > 0 {
            text.push_str(&format!(" {} folders could not be read.", status.skipped));
        }
        self.status.set_text(&text);
    }

    /// Cancel the running search (if any) without touching the entry.
    pub fn end_search(&self) {
        let run = self.search.borrow_mut().take();
        if let Some(run) = run {
            run.search.cancel();
            // Its results are gone: nothing selected there can still be opened.
            self.search_selection_changed(None);
        }
    }

    /// Navigation and tab switches leave search: cancel it and clear the entry silently.
    pub fn leave_search(&self) {
        if self.is_searching() || !self.search_entry.text().is_empty() {
            self.end_search();
            self.search_clearing.set(true);
            self.search_entry.set_text("");
            self.search_clearing.set(false);
        }
    }

    /// The results list: Name, Folder and Type, in discovery order.
    pub fn search_view(self: &Rc<Self>, generation: u64) -> Option<gtk::Widget> {
        let store = self.search.borrow().as_ref()?.store.clone();
        let selection = gtk::SingleSelection::new(Some(store));
        selection.set_autoselect(false);
        selection.set_can_unselect(true);
        // A rebuilt view starts without a selection, so a pending explicit Open is dropped.
        self.search_selection_changed(None);
        let weak = Rc::downgrade(self);
        selection.connect_selected_notify(move |selection| {
            if let Some(ui) = weak.upgrade().filter(|ui| ui.render_generation.get() == generation) {
                ui.search_selection_changed(selection.selected_item().and_downcast::<glib::BoxedAnyObject>().map(|object| object.borrow::<Entry>().clone()));
            }
        });
        let slot: super::open_ui::SelectionSlot = Rc::new(std::cell::RefCell::new(Some(selection.clone())));
        let row_menu = self.row_menu(slot, generation);
        let view = gtk::ColumnView::new(Some(selection.clone()));
        view.update_property(&[gtk::accessible::Property::Label("Search results")]);
        self.attach_open_keys(&view, generation);
        for (title, kind) in [("Name", 0u8), ("Folder", 4), ("Type", 1)] {
            let column = gtk::ColumnViewColumn::new(Some(title), Some(super::factory(kind, Some(row_menu.clone()))));
            column.set_expand(kind != 1);
            column.set_resizable(true);
            view.append_column(&column);
        }
        let weak = Rc::downgrade(self);
        view.connect_activate(move |_, position| {
            let Some(ui) = weak.upgrade().filter(|ui| ui.render_generation.get() == generation) else {
                return;
            };
            if let Some(entry) = super::entry_at(&selection, position) {
                ui.reveal_result(entry);
            }
        });
        let scroll = gtk::ScrolledWindow::builder().vexpand(true).hexpand(true).child(&view).build();
        self.content.append(&scroll);
        self.search_status();
        Some(view.upcast())
    }

    /// A folder result opens it; a file result opens its folder with the file selected.
    fn reveal_result(self: &Rc<Self>, entry: Entry) {
        self.leave_search();
        if entry.directory {
            self.navigate(entry.path, Intent::Visit);
        } else if let Some(parent) = entry.path.parent().map(std::path::Path::to_path_buf) {
            self.ops.select_after_refresh.replace(Some(entry.path));
            self.navigate(parent, Intent::Visit);
        }
    }

    pub fn search_evidence(&self) -> serde_json::Value {
        let search = self.search.borrow();
        let Some(run) = search.as_ref() else {
            return serde_json::json!({"active":false, "query":self.search_entry.text().as_str()});
        };
        let results: Vec<_> = (0..run.store.n_items().min(256)).filter_map(|index| super::entry_at(&run.store, index).map(|entry| entry.path.to_string_lossy().into_owned())).collect();
        serde_json::json!({"active":true, "id":run.id, "query":run.query, "root":run.root.to_string_lossy(), "results":results, "count":run.store.n_items(),
            "done":run.status.done, "truncated":run.status.truncated, "visited":run.status.visited})
    }
}

#[cfg(not(feature = "fixture-delay"))]
fn start(root: PathBuf, query: &str) -> std::io::Result<Search> {
    Search::start(root, query)
}

/// Fixture builds only: a capped delay per folder makes cancellation observable.
#[cfg(feature = "fixture-delay")]
fn start(root: PathBuf, query: &str) -> std::io::Result<Search> {
    match std::env::var("DECFILES_FIXTURE_SEARCH_DELAY_MS").ok().and_then(|value| value.parse::<u64>().ok()).filter(|delay| *delay > 0) {
        Some(delay) => Search::start_throttled(root, query, Duration::from_millis(delay.min(2_000))),
        None => Search::start(root, query),
    }
}
