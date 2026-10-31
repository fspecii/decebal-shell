//! Cut/Copy/Paste with GNOME Files-compatible clipboard formats. Copy and move work runs on
//! the transfer worker; this module only polls its snapshot and answers conflict prompts.
use super::Ui;
use decfiles::transfer::{self, clipboard, Choice, Mode, Phase, Report, Request, Transfer};
use gio::prelude::*;
use gtk::prelude::*;
use std::{path::PathBuf, rc::Rc, time::Duration};

const POLL: Duration = Duration::from_millis(50);

fn leaf(path: &std::path::Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into_owned())
}

impl Ui {
    pub fn connect_transfers(self: &Rc<Self>) {
        for (button, mode) in [(&self.ops.cut, Mode::Move), (&self.ops.copy, Mode::Copy)] {
            let weak = Rc::downgrade(self);
            button.connect_clicked(move |_| {
                if let Some(ui) = weak.upgrade() {
                    ui.clipboard_put(mode);
                }
            });
        }
        let weak = Rc::downgrade(self);
        self.ops.paste.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.paste();
            }
        });
    }

    /// Publish the selected item. Nothing on disk changes until Paste.
    pub fn clipboard_put(self: &Rc<Self>, mode: Mode) {
        if !self.can_start_dialog() {
            return;
        }
        let Some(path) = self.ops.active.borrow().clone() else {
            return;
        };
        let paths = [path.clone()];
        let bytes = |text: String| glib::Bytes::from_owned(text.into_bytes());
        let plain = paths.iter().map(|path| path.to_string_lossy()).collect::<Vec<_>>().join("\n");
        let provider = gtk::gdk::ContentProvider::new_union(&[
            gtk::gdk::ContentProvider::for_bytes(clipboard::GNOME, &bytes(clipboard::encode_gnome(mode, &paths))),
            gtk::gdk::ContentProvider::for_bytes(clipboard::URI_LIST, &bytes(clipboard::encode_uri_list(&paths))),
            gtk::gdk::ContentProvider::for_bytes("text/plain;charset=utf-8", &bytes(plain)),
        ]);
        if self.window.clipboard().set_content(Some(&provider)).is_err() {
            self.status.set_text("Could not place the item on the clipboard.");
            return;
        }
        use std::os::unix::ffi::OsStrExt;
        self.ops.clipboard_state.replace(Some(serde_json::json!({"mode":mode, "paths":[path.to_string_lossy()], "paths_bytes":[path.as_os_str().as_bytes()]})));
        self.status.set_text(&match mode {
            Mode::Copy => format!("Copied “{}”. Paste in a folder to make a copy.", leaf(&path)),
            Mode::Move => format!("Cut “{}”. Paste in another folder to move it.", leaf(&path)),
        });
        self.dump();
    }

    /// Read files from the clipboard asynchronously, then copy or move them here.
    pub fn paste(self: &Rc<Self>) {
        if !self.can_start_dialog() || self.ops.paste_reading.get() {
            return;
        }
        let Some(destination) = self.model.borrow().current().map(std::path::Path::to_path_buf) else {
            return;
        };
        self.ops.paste_reading.set(true);
        self.update_operation_controls();
        let weak = Rc::downgrade(self);
        self.window.clipboard().read_async(&[clipboard::GNOME, clipboard::URI_LIST], glib::Priority::DEFAULT, gio::Cancellable::NONE, move |result| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let Ok((stream, mime)) = result else {
                ui.paste_failed("The clipboard does not contain files.");
                return;
            };
            let weak = Rc::downgrade(&ui);
            drop(ui);
            glib::MainContext::default().spawn_local(async move {
                let mut data = Vec::new();
                let complete = loop {
                    match stream.read_bytes_future(64 * 1024, glib::Priority::DEFAULT).await {
                        Ok(chunk) if chunk.is_empty() => break true,
                        Ok(chunk) if data.len() + chunk.len() <= clipboard::MAX_BYTES => data.extend_from_slice(&chunk),
                        _ => break false,
                    }
                };
                let _ = stream.close_future(glib::Priority::DEFAULT).await;
                let Some(ui) = weak.upgrade() else {
                    return;
                };
                let text = String::from_utf8_lossy(&data);
                let decoded = if complete { clipboard::decode(&mime, &text) } else { None };
                match decoded {
                    Some((mode, sources)) => {
                        ui.ops.paste_reading.set(false);
                        ui.start_transfer(Request { mode, sources, destination, policy: None });
                    }
                    None => ui.paste_failed("The clipboard does not contain local files that can be pasted."),
                }
            });
        });
    }

    fn paste_failed(&self, message: &str) {
        self.ops.paste_reading.set(false);
        self.status.set_text(message);
        self.update_operation_controls();
        self.dump();
    }

    fn start_transfer(self: &Rc<Self>, request: Request) {
        if !self.can_start_dialog() {
            return;
        }
        if let Err(message) = transfer::validate(&request) {
            self.model.borrow_mut().error = Some(message);
            self.render();
            return;
        }
        let mode = request.mode;
        let started = start(request);
        let transfer = match started {
            Ok(transfer) => Rc::new(transfer),
            Err(error) => {
                self.model.borrow_mut().error = Some(format!("Could not start the transfer: {error}"));
                self.render();
                return;
            }
        };
        let id = self.ops.next_id.get().wrapping_add(1);
        self.ops.next_id.set(id);
        let cancel = gio::Cancellable::new();
        // The existing Cancel operation and close confirmation cancel this GCancellable.
        let cancel_transfer = transfer.canceller();
        let _ = cancel.connect_cancelled(move |_| cancel_transfer());
        self.ops.running.replace(Some(super::operation_ui::Running { id, cancel, transfer: Some(transfer) }));
        self.ops.notice.borrow_mut().take();
        self.model.borrow_mut().error = None;
        self.error.set_visible(false);
        self.ops.progress_row.set_visible(true);
        self.ops.progress_label.set_text(if mode == Mode::Copy { "Preparing to copy…" } else { "Preparing to move…" });
        self.ops.progress.set_fraction(0.0);
        self.ops.progress.set_text(Some(""));
        self.status.set_text("Working… You can cancel this operation.");
        self.update_operation_controls();
        self.dump();
        let weak = Rc::downgrade(self);
        glib::timeout_add_local(POLL, move || {
            let Some(ui) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            ui.poll_transfer(id, mode)
        });
    }

    fn poll_transfer(self: &Rc<Self>, id: u64, mode: Mode) -> glib::ControlFlow {
        let transfer = match self.ops.running.borrow().as_ref() {
            Some(run) if run.id == id => run.transfer.clone(),
            _ => None,
        };
        let Some(transfer) = transfer else {
            return glib::ControlFlow::Break;
        };
        let snapshot = transfer.poll();
        let progress = &snapshot.progress;
        let verb = if mode == Mode::Copy { "Copying" } else { "Moving" };
        let label = match (&progress.phase, &progress.current) {
            (Phase::Preparing, _) => format!("{} — counting items…", if mode == Mode::Copy { "Copy" } else { "Move" }),
            (Phase::WaitingForChoice, _) => "Waiting for your choice…".into(),
            (_, Some(name)) => format!("{verb} “{name}”"),
            _ => format!("{verb}…"),
        };
        self.ops.progress_label.set_text(&label);
        match progress.fraction() {
            Some(fraction) => self.ops.progress.set_fraction(fraction),
            None => self.ops.progress.pulse(),
        }
        self.ops.progress.set_text(Some(&format!("{} of {} items", progress.items_done.min(progress.items_total), progress.items_total)));
        self.ops.transfer_state.replace(Some(serde_json::json!({"id":id, "mode":mode, "progress":progress, "conflict":snapshot.conflict})));
        if let Some(report) = snapshot.report {
            self.transfer_finished(id, report);
            return glib::ControlFlow::Break;
        }
        if let Some(conflict) = snapshot.conflict {
            if self.ops.dialog.borrow().is_none() {
                self.conflict_dialog(&transfer, conflict);
            }
        }
        self.update_operation_controls();
        self.dump();
        glib::ControlFlow::Continue
    }

    fn conflict_dialog(self: &Rc<Self>, transfer: &Rc<Transfer>, conflict: transfer::Conflict) {
        let dialog = gtk::Dialog::builder().transient_for(&self.window).modal(true).destroy_with_parent(true).title("Item already exists").default_width(500).build();
        dialog.add_button("Stop", gtk::ResponseType::Cancel);
        let skip = dialog.add_button("Skip", gtk::ResponseType::Reject);
        dialog.add_button("Keep Both", gtk::ResponseType::Accept);
        dialog.set_default_response(gtk::ResponseType::Reject);
        let body = dialog.content_area();
        body.set_spacing(10);
        body.set_margin_top(18);
        body.set_margin_bottom(18);
        body.set_margin_start(18);
        body.set_margin_end(18);
        let kind = if conflict.directory { "A folder" } else { "An item" };
        let text = format!(
            "{kind} named “{}” already exists in “{}”.\n\nSkip leaves both unchanged. Keep Both adds “(copy)” to the new name. Existing items are never replaced.",
            leaf(&conflict.destination),
            conflict.destination.parent().map_or_else(String::new, |parent| parent.display().to_string())
        );
        let label = gtk::Label::new(Some(&text));
        label.set_wrap(true);
        label.set_xalign(0.0);
        label.set_selectable(true);
        body.append(&label);
        let all = gtk::CheckButton::with_label("Apply to all remaining conflicts");
        body.append(&all);
        self.ops.dialog.replace(Some(dialog.clone()));
        self.dump();
        let weak = Rc::downgrade(self);
        let transfer = transfer.clone();
        let id = conflict.id;
        dialog.connect_response(move |dialog, response| {
            // Escape and window close mean Stop, the conservative choice.
            let choice = match response {
                gtk::ResponseType::Reject => Choice::Skip,
                gtk::ResponseType::Accept => Choice::KeepBoth,
                _ => Choice::Stop,
            };
            transfer.answer(id, choice, all.is_active());
            if let Some(ui) = weak.upgrade() {
                ui.ops.dialog.borrow_mut().take();
                ui.dump();
            }
            dialog.destroy();
        });
        dialog.present();
        skip.grab_focus();
    }

    fn transfer_finished(self: &Rc<Self>, id: u64, report: Report) {
        self.ops.running.borrow_mut().take();
        let dialog = self.ops.dialog.borrow_mut().take();
        if let Some(dialog) = dialog {
            dialog.destroy();
        }
        self.ops.progress_row.set_visible(false);
        let status = if report.cancelled {
            "cancelled"
        } else if report.failure_count > 0 && report.completed == 0 {
            "error"
        } else if report.failure_count > 0 || !report.notes.is_empty() {
            "partial"
        } else {
            "success"
        };
        self.ops.last_result.replace(Some(serde_json::json!({"id":id, "status":status, "kind":"Transfer", "message":report.message, "report":report})));
        // A completed cut is consumed, like GNOME Files; a partial one stays for retry.
        if report.mode == Mode::Move && status == "success" && self.ops.clipboard_state.borrow().as_ref().is_some_and(|state| state["mode"] == "Move") {
            let _ = self.window.clipboard().set_content(None::<&gtk::gdk::ContentProvider>);
            self.ops.clipboard_state.replace(None);
        }
        let Some(destination) = self.model.borrow().current().map(std::path::Path::to_path_buf) else {
            self.render();
            return;
        };
        if status != "success" && report.changed {
            self.ops.error_after_refresh.replace(Some(report.message.clone()));
        }
        if !report.changed {
            if status == "error" || status == "partial" {
                self.model.borrow_mut().error = Some(report.message.clone());
            }
            self.ops.notice.replace(Some(report.message));
            self.render();
            return;
        }
        let selected: Option<PathBuf> = report.created.first().cloned();
        self.refresh_operation(destination, selected, report.message);
    }
}

#[cfg(not(feature = "fixture-delay"))]
fn start(request: Request) -> std::io::Result<Transfer> {
    Transfer::start(request)
}

/// Fixture builds only: a capped per-chunk delay makes cancellation observable.
#[cfg(feature = "fixture-delay")]
fn start(request: Request) -> std::io::Result<Transfer> {
    match std::env::var("DECFILES_FIXTURE_TRANSFER_DELAY_MS").ok().and_then(|value| value.parse::<u64>().ok()).filter(|delay| *delay > 0) {
        Some(delay) => Transfer::start_throttled(request, Duration::from_millis(delay.min(5_000))),
        None => Transfer::start(request),
    }
}
