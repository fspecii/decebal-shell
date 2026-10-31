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

pub struct Theme {
    pub gtk_css: PathBuf,
    pub palette: gtk::CssProvider,
    pub palette_text: std::cell::RefCell<Option<String>>,
    pub reloads: std::cell::Cell<u64>,
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

    /// Asynchronous read; a missing file means no palette override and the Native profile.
    fn reload_theme(self: &Rc<Self>) {
        let file = gio::File::for_path(&self.theme.gtk_css);
        let weak = Rc::downgrade(self);
        glib::MainContext::default().spawn_local(async move {
            let text = match file.load_contents_future().await {
                Ok((bytes, _)) if bytes.len() <= MAX_CSS => String::from_utf8_lossy(&bytes).into_owned(),
                Ok(_) => return,
                Err(_) => String::new(),
            };
            let Some(ui) = weak.upgrade() else {
                return;
            };
            if ui.theme.palette_text.borrow().as_deref() != Some(text.as_str()) {
                ui.theme.palette.load_from_data(&text);
                ui.theme.palette_text.replace(Some(text.clone()));
                ui.theme.reloads.set(ui.theme.reloads.get() + 1);
            }
            if !ui.theme.from_cli {
                ui.apply_profile(Profile::from_generated_css(&text));
            }
            ui.dump();
        });
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
            "palette_reloads":self.theme.reloads.get(), "gtk_css":self.theme.gtk_css.to_string_lossy(),
            "window_classes":self.window.css_classes().iter().map(|class| class.to_string()).collect::<Vec<_>>()})
    }
}
