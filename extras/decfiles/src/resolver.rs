//! One filesystem worker and replaceable request/result slots. Resolving a physical
//! path must precede GLocalFile construction: GLocalFile collapses `link/..` lexically.
use crate::model::Request;
use std::{
    io::{self, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    sync::{Arc, Condvar, Mutex},
    thread,
};

pub struct Resolved {
    pub request: Request,
    pub result: Result<PathBuf, String>,
}
#[derive(Default)]
struct State {
    pending: Option<Request>,
    result: Option<Resolved>,
    stopped: bool,
}
pub struct Resolver {
    shared: Arc<(Mutex<State>, Condvar)>,
}
impl Resolver {
    pub fn new() -> io::Result<(Self, UnixStream)> {
        let (reader, mut writer) = UnixStream::pair()?;
        reader.set_nonblocking(true)?;
        writer.set_nonblocking(true)?;
        let shared = Arc::new((Mutex::new(State::default()), Condvar::new()));
        let worker = shared.clone();
        thread::Builder::new().name("decfiles-path".into()).spawn(move || {
            loop {
                let (lock, wake) = &*worker;
                let request = {
                    let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
                    while !state.stopped && state.pending.is_none() {
                        state = wake.wait(state).unwrap_or_else(|e| e.into_inner());
                    }
                    if state.stopped {
                        return;
                    }
                    state.pending.take().unwrap()
                };
                let result = std::fs::canonicalize(&request.path).map_err(|error| format!("Could not resolve this folder: {error}"));
                {
                    let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
                    if state.stopped {
                        return;
                    }
                    state.result = Some(Resolved { request, result });
                }
                // Results are latest-only. A full wake socket is already readable.
                let _ = writer.write(&[1]);
            }
        })?;
        Ok((Self { shared }, reader))
    }
    pub fn request(&self, request: Request) {
        let (lock, wake) = &*self.shared;
        let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
        if !state.stopped {
            state.pending = Some(request);
            wake.notify_one();
        }
    }
    pub fn take_result(&self) -> Option<Resolved> {
        self.shared.0.lock().unwrap_or_else(|e| e.into_inner()).result.take()
    }
    pub fn stop(&self) {
        let (lock, wake) = &*self.shared;
        let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
        state.stopped = true;
        state.pending = None;
        state.result = None;
        wake.notify_one();
    }
}
impl Drop for Resolver {
    fn drop(&mut self) {
        self.stop();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Intent;
    use std::{
        os::unix::fs::symlink,
        time::{Duration, Instant},
    };
    #[test]
    fn resolves_real_symlink_parent_identity_off_the_caller_thread() {
        let base = std::env::temp_dir().join(format!("decfiles-resolver-{}", std::process::id()));
        let logical = base.join("logical");
        let physical = base.join("physical");
        std::fs::create_dir_all(&logical).unwrap();
        std::fs::create_dir_all(physical.join("nested")).unwrap();
        symlink(physical.join("nested"), logical.join("link")).unwrap();
        let (resolver, _reader) = Resolver::new().unwrap();
        resolver.request(Request { revision: 1, path: logical.join("link/.."), intent: Intent::Visit });
        let until = Instant::now() + Duration::from_secs(5);
        let result = loop {
            if let Some(result) = resolver.take_result() {
                break result;
            }
            assert!(Instant::now() < until);
            thread::sleep(Duration::from_millis(2));
        };
        assert_eq!(result.result.unwrap(), physical);
        resolver.stop();
        std::fs::remove_dir_all(base).unwrap();
    }
}
