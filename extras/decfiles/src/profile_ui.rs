//! Live palette and profile following. GTK reads the user's `gtk-4.0/gtk.css` once at
//! startup; a bounded folder monitor notices the shell rewriting it, the file is read
//! asynchronously, and a USER+1 provider applies the new palette. The profile marker
//! picks Explorer/Finder/Native surfaces and the default layout, never an explicit one.
use super::Ui;
use decfiles::{monitor::Monitor, profile::Profile};
use gio::prelude::*;
use gtk::prelude::*;
use std::{io::Read, os::fd::AsRawFd, path::PathBuf, rc::Rc, time::Duration};

const MAX_CSS: usize = 2 * 1024 * 1024;

enum ThemeRead {
    Text(String),
    Missing,
    TooLarge,
    Rejected,
    Failed,
}

pub struct Theme {
    pub gtk_css: PathBuf,
    pub palette: gtk::CssProvider,
    pub palette_text: std::cell::RefCell<Option<String>>,
    pub reloads: std::cell::Cell<u64>,
    /// Latest read generation; older completions are discarded, never applied.
    pub generation: std::cell::Cell<u64>,
    pub stale_ignored: std::cell::Cell<u64>,
    pub too_large: std::cell::Cell<u64>,
    pub rejected: std::cell::Cell<u64>,
    /// One physical read at most: `active` while it has not returned, `pending` for the
    /// single coalesced newer request.
    pub active: std::cell::Cell<bool>,
    pub pending: std::cell::Cell<bool>,
    pub closed: std::cell::Cell<bool>,
    pub cancel: std::cell::RefCell<Option<gio::Cancellable>>,
    pub reads_started: std::cell::Cell<u64>,
    pub requests: std::cell::Cell<u64>,
    #[cfg(feature = "fixture-delay")]
    pub held: std::cell::Cell<bool>,
    pub from_cli: bool,
    pub monitor: Option<Monitor>,
    pub watching: std::cell::RefCell<Option<PathBuf>>,
    pub source: std::cell::RefCell<Option<glib::SourceId>>,
    pub timer: std::cell::RefCell<Option<glib::SourceId>>,
}

impl Theme {
    pub fn new(from_cli: bool) -> (Self, Option<std::os::unix::net::UnixStream>) {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
            .unwrap_or_else(|| PathBuf::from("/nonexistent"));
        let (monitor, reader) = Monitor::new().map_or((None, None), |(monitor, reader)| (Some(monitor), Some(reader)));
        let theme = Self {
            gtk_css: config.join("gtk-4.0/gtk.css"),
            palette: gtk::CssProvider::new(),
            palette_text: Default::default(),
            reloads: Default::default(),
            generation: Default::default(),
            stale_ignored: Default::default(),
            too_large: Default::default(),
            rejected: Default::default(),
            active: Default::default(),
            pending: Default::default(),
            closed: Default::default(),
            cancel: Default::default(),
            reads_started: Default::default(),
            requests: Default::default(),
            #[cfg(feature = "fixture-delay")]
            held: Default::default(),
            from_cli,
            monitor,
            watching: Default::default(),
            source: Default::default(),
            timer: Default::default(),
        };
        (theme, reader)
    }
}

impl Ui {
    pub fn start_theme(self: &Rc<Self>, reader: Option<std::os::unix::net::UnixStream>) {
        let display = gtk::prelude::WidgetExt::display(&self.window);
        // The user's own gtk.css stays above our profile surfaces; the reloaded palette
        // replaces GTK's startup copy of that same file.
        let profile = gtk::CssProvider::new();
        profile.load_from_data(decfiles::profile::css());
        gtk::style_context_add_provider_for_display(&display, &profile, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
        gtk::style_context_add_provider_for_display(&display, &self.theme.palette, gtk::STYLE_PROVIDER_PRIORITY_USER + 1);
        self.window.add_css_class(self.profile.get().css_class());
        if let Some(mut reader) = reader {
            let weak = Rc::downgrade(self);
            let source = glib::source::unix_fd_add_local(reader.as_raw_fd(), glib::IOCondition::IN | glib::IOCondition::HUP, move |_, condition| {
                let Some(ui) = weak.upgrade() else {
                    return glib::ControlFlow::Break;
                };
                let mut bytes = [0; 128];
                while matches!(reader.read(&mut bytes), Ok(n) if n > 0) {}
                if ui.theme.monitor.as_ref().and_then(Monitor::take_change).is_some() {
                    ui.theme_changed();
                }
                if condition.contains(glib::IOCondition::HUP) {
                    glib::ControlFlow::Break
                } else {
                    glib::ControlFlow::Continue
                }
            });
            self.theme.source.replace(Some(source));
        }
        self.watch_theme();
        self.reload_theme();
    }

    /// Watch gtk-4.0, or its parent until the shell first creates it.
    fn watch_theme(&self) {
        let folder = self.theme.gtk_css.parent().map(std::path::Path::to_path_buf).unwrap_or_default();
        let target = if folder.is_dir() { folder } else { folder.parent().map(std::path::Path::to_path_buf).unwrap_or_default() };
        if self.theme.watching.borrow().as_ref() != Some(&target) {
            if let Some(monitor) = &self.theme.monitor {
                monitor.watch(1, target.clone());
            }
            self.theme.watching.replace(Some(target));
        }
    }

    fn theme_changed(self: &Rc<Self>) {
        if self.theme.timer.borrow().is_some() {
            return;
        }
        let weak = Rc::downgrade(self);
        let timer = glib::timeout_add_local_once(Duration::from_millis(200), move || {
            if let Some(ui) = weak.upgrade() {
                ui.theme.timer.borrow_mut().take();
                ui.watch_theme();
                ui.reload_theme();
            }
        });
        self.theme.timer.replace(Some(timer));
    }

    /// Request a re-read. At most one physical read is ever outstanding: a request during a
    /// read cancels that (now stale) read's GIO operations and leaves one coalesced pending
    /// request, started only after the current read has actually returned.
    fn reload_theme(self: &Rc<Self>) {
        let theme = &self.theme;
        if theme.closed.get() {
            return;
        }
        theme.requests.set(theme.requests.get() + 1);
        if theme.active.get() {
            theme.pending.set(true);
            if let Some(cancel) = theme.cancel.borrow().as_ref() {
                cancel.cancel();
            }
            self.dump();
            return;
        }
        self.start_read();
    }

    fn start_read(self: &Rc<Self>) {
        let theme = &self.theme;
        let generation = theme.generation.get() + 1;
        theme.generation.set(generation);
        theme.active.set(true);
        theme.pending.set(false);
        theme.reads_started.set(theme.reads_started.get() + 1);
        let cancel = gio::Cancellable::new();
        theme.cancel.replace(Some(cancel.clone()));
        self.dump();
        let file = gio::File::for_path(&theme.gtk_css);
        let weak = Rc::downgrade(self);
        // Reject non-regular inputs (FIFOs, devices, folders) and oversized files before any open.
        file.clone().query_info_async("standard::type,standard::size", gio::FileQueryInfoFlags::NONE, glib::Priority::DEFAULT, Some(&cancel.clone()), move |info| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            match info {
                Err(error) if error.matches(gio::IOErrorEnum::NotFound) => ui.read_finished(generation, ThemeRead::Missing),
                Err(_) => ui.read_finished(generation, ThemeRead::Failed),
                Ok(info) if info.file_type() != gio::FileType::Regular => ui.read_finished(generation, ThemeRead::Rejected),
                Ok(info) if info.size() > MAX_CSS as i64 => ui.read_finished(generation, ThemeRead::TooLarge),
                Ok(_) => {
                    let weak = Rc::downgrade(&ui);
                    file.read_async(glib::Priority::DEFAULT, Some(&cancel.clone()), move |stream| {
                        let Some(ui) = weak.upgrade() else {
                            return;
                        };
                        match stream {
                            Ok(stream) => ui.read_chunk(generation, stream.upcast(), Vec::new(), cancel),
                            Err(_) => ui.read_finished(generation, ThemeRead::Failed),
                        }
                    });
                }
            }
        });
    }

    /// Ask for at most the remaining allowance plus one sentinel byte, so oversize is
    /// detected after reading at most MAX_CSS + 1 bytes.
    fn read_chunk(self: &Rc<Self>, generation: u64, stream: gio::InputStream, data: Vec<u8>, cancel: gio::Cancellable) {
        let want = (MAX_CSS + 1 - data.len()).min(64 * 1024);
        let weak = Rc::downgrade(self);
        stream.clone().read_bytes_async(want, glib::Priority::DEFAULT, Some(&cancel.clone()), move |chunk| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let mut data = data;
            let outcome = match chunk {
                Ok(chunk) if chunk.is_empty() => Some(ThemeRead::Text(String::from_utf8_lossy(&data).into_owned())),
                Ok(chunk) => {
                    data.extend_from_slice(&chunk);
                    (data.len() > MAX_CSS).then_some(ThemeRead::TooLarge)
                }
                Err(_) => Some(ThemeRead::Failed),
            };
            match outcome {
                None => ui.read_chunk(generation, stream, data, cancel),
                Some(read) => {
                    stream.close_async(glib::Priority::DEFAULT, gio::Cancellable::NONE, |_| {});
                    ui.read_finished(generation, read);
                }
            }
        });
    }

    /// The physical read has returned. Apply it only if no newer request is pending;
    /// otherwise discard it and start the one coalesced read.
    fn read_finished(self: &Rc<Self>, generation: u64, read: ThemeRead) {
        #[cfg(feature = "fixture-delay")]
        if let Some(delay) = std::env::var("DECFILES_FIXTURE_THEME_DELAY_MS").ok().and_then(|value| value.parse::<u64>().ok()).filter(|delay| *delay > 0) {
            if !self.theme.held.replace(true) {
                // Fixture builds only: model a read blocked in a syscall that cancellation
                // cannot retire. The read slot stays occupied for the whole hold.
                let weak = Rc::downgrade(self);
                glib::timeout_add_local_once(Duration::from_millis(delay.min(10_000)), move || {
                    if let Some(ui) = weak.upgrade() {
                        ui.read_finished(generation, read);
                    }
                });
                return;
            }
            self.theme.held.set(false);
        }
        let theme = &self.theme;
        theme.active.set(false);
        theme.cancel.borrow_mut().take();
        if theme.closed.get() {
            return;
        }
        if theme.pending.get() || theme.generation.get() != generation {
            if matches!(read, ThemeRead::Text(_) | ThemeRead::Missing) {
                theme.stale_ignored.set(theme.stale_ignored.get() + 1);
            }
            self.start_read();
            return;
        }
        let text = match read {
            ThemeRead::Text(text) => text,
            ThemeRead::Missing => String::new(),
            ThemeRead::TooLarge => {
                theme.too_large.set(theme.too_large.get() + 1);
                self.dump();
                return;
            }
            ThemeRead::Rejected => {
                theme.rejected.set(theme.rejected.get() + 1);
                self.dump();
                return;
            }
            ThemeRead::Failed => {
                self.dump();
                return;
            }
        };
        if theme.palette_text.borrow().as_deref() != Some(text.as_str()) {
            theme.palette.load_from_data(&text);
            theme.palette_text.replace(Some(text.clone()));
            theme.reloads.set(theme.reloads.get() + 1);
        }
        if !theme.from_cli {
            self.apply_profile(Profile::from_generated_css(&text));
        }
        self.dump();
    }

    /// Window closing: cancel the active read, drop any pending request, start nothing new.
    /// A syscall already blocked inside GIO's worker cannot be forced out; it is not joined.
    pub fn close_theme(&self) {
        let theme = &self.theme;
        theme.closed.set(true);
        theme.pending.set(false);
        if let Some(cancel) = theme.cancel.borrow().as_ref() {
            cancel.cancel();
        }
        if let Some(monitor) = &theme.monitor {
            monitor.stop();
        }
        for source in [theme.source.borrow_mut().take(), theme.timer.borrow_mut().take()].into_iter().flatten() {
            source.remove();
        }
    }

    /// Profile surfaces and vocabulary; the default layout only when none was chosen.
    pub fn apply_profile(self: &Rc<Self>, profile: Profile) {
        let changed = self.profile.get() != profile;
        if changed {
            self.window.remove_css_class(self.profile.get().css_class());
            self.window.add_css_class(profile.css_class());
            self.profile.set(profile);
        }
        let default_layout = !self.layout_explicit.get() && self.columns.get() != profile.prefers_columns();
        if !self.layout_explicit.get() {
            for tab in self.tabs.borrow_mut().iter_mut() {
                tab.columns = profile.prefers_columns();
            }
        }
        if default_layout {
            self.set_columns(profile.prefers_columns());
        }
        if changed || default_layout {
            self.render();
        }
    }

    pub fn profile_evidence(&self) -> serde_json::Value {
        serde_json::json!({"id":self.profile.get().id(), "from_cli":self.theme.from_cli, "layout_explicit":self.layout_explicit.get(),
            "palette_reloads":self.theme.reloads.get(), "generation":self.theme.generation.get(), "stale_ignored":self.theme.stale_ignored.get(), "too_large":self.theme.too_large.get(),
            "rejected":self.theme.rejected.get(), "active_read":self.theme.active.get(), "pending_read":self.theme.pending.get(),
            "reads_started":self.theme.reads_started.get(), "requests":self.theme.requests.get(), "closed":self.theme.closed.get(), "gtk_css":self.theme.gtk_css.to_string_lossy(),
            "window_classes":self.window.css_classes().iter().map(|class| class.to_string()).collect::<Vec<_>>()})
    }
}
