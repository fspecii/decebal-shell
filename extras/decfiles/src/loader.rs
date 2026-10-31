use crate::model::{Entry, MAX_ENTRIES};
use gio::prelude::*;
use std::{cell::RefCell, rc::Rc};

const BATCH: i32 = 128;
type Done = Box<dyn FnOnce(Result<Vec<Entry>, String>)>;

struct Job {
    enumerator: gio::FileEnumerator,
    cancel: gio::Cancellable,
    entries: RefCell<Vec<Entry>>,
    done: RefCell<Option<Done>>,
}

pub fn load(path: &std::path::Path, cancel: &gio::Cancellable, done: impl FnOnce(Result<Vec<Entry>, String>) + 'static) {
    let file = gio::File::for_path(path);
    let cancel_owned = cancel.clone();
    file.enumerate_children_async(
        "standard::name,standard::display-name,standard::type,standard::size,time::modified,standard::fast-content-type,standard::is-symlink",
        gio::FileQueryInfoFlags::NONE,
        glib::Priority::DEFAULT,
        Some(cancel),
        move |result| match result {
            Ok(enumerator) => next(Rc::new(Job { enumerator, cancel: cancel_owned, entries: RefCell::new(Vec::new()), done: RefCell::new(Some(Box::new(done))) })),
            Err(error) => done(Err(format!("Could not open this folder: {error}"))),
        },
    );
}

fn finish(job: &Job, result: Result<Vec<Entry>, String>) {
    job.enumerator.close_async(glib::Priority::DEFAULT, gio::Cancellable::NONE, |_| {});
    if let Some(done) = job.done.borrow_mut().take() {
        done(result);
    }
}

fn next(job: Rc<Job>) {
    let owned = job.clone();
    job.enumerator.next_files_async(BATCH, glib::Priority::DEFAULT, Some(&job.cancel), move |result| match result {
        Err(error) => finish(&owned, Err(format!("Could not read this folder: {error}"))),
        Ok(infos) if infos.is_empty() => {
            let entries = std::mem::take(&mut *owned.entries.borrow_mut());
            finish(&owned, Ok(entries));
        }
        Ok(infos) => {
            if owned.entries.borrow().len() + infos.len() > MAX_ENTRIES {
                finish(&owned, Err(format!("This prototype supports at most {MAX_ENTRIES} items per folder. The previous folder is unchanged.")));
                return;
            }
            for info in infos {
                if let Some(path) = owned.enumerator.child(&info).path() {
                    owned.entries.borrow_mut().push(Entry {
                        path,
                        name: info.display_name().to_string(),
                        directory: info.file_type() == gio::FileType::Directory,
                        size: info.size().max(0) as u64,
                        modified: info.attribute_uint64("time::modified"),
                        content_type: info.attribute_string("standard::fast-content-type").map(|value| value.to_string()).unwrap_or_default(),
                        symlink: info.is_symlink(),
                    });
                }
            }
            next(owned);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, os::unix::ffi::OsStringExt};
    fn collect(path: &std::path::Path, cancelled: bool) -> Result<Vec<Entry>, String> {
        let context = glib::MainContext::new();
        context
            .with_thread_default(|| {
                let loop_ = glib::MainLoop::new(Some(&context), false);
                let result = Rc::new(RefCell::new(None));
                let cancel = gio::Cancellable::new();
                if cancelled {
                    cancel.cancel();
                }
                let output = result.clone();
                let loop_clone = loop_.clone();
                load(path, &cancel, move |value| {
                    output.replace(Some(value));
                    loop_clone.quit();
                });
                loop_.run();
                let value = result.borrow_mut().take().unwrap();
                value
            })
            .unwrap()
    }
    #[test]
    fn reads_actual_names_in_batches_and_handles_missing_and_cancelled_folders() {
        let path = std::env::temp_dir().join(format!("decfiles-loader-{}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        std::fs::create_dir(path.join("folder # % ü")).unwrap();
        for i in 0..300 {
            std::fs::write(path.join(format!("file {i}")), b"bytes").unwrap();
        }
        let raw_name = std::ffi::OsString::from_vec(b"non-utf8-\xff".to_vec());
        let raw_path = path.join(&raw_name);
        std::fs::write(&raw_path, b"raw").unwrap();
        let entries = collect(&path, false).unwrap();
        assert_eq!(entries.len(), 302);
        assert!(entries.iter().any(|entry| entry.path == raw_path));
        assert!(entries.iter().any(|entry| entry.name == "folder # % ü" && entry.directory));
        assert!(entries.iter().any(|entry| entry.size == 5));
        assert!(collect(&path.join("missing"), false).is_err());
        assert!(collect(&path, true).is_err());
        std::fs::remove_dir_all(path).unwrap();
    }
}
