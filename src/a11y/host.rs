//! The UI-thread side of the bridge, embedded by the window backends.
//!
//! A backend owns one [`Host`] next to its window and calls it at three points: [`Host::service`] when its event
//! loop is woken, [`Host::after_draw`] after each paint, and [`Host::set_window`] when the window moves, gains or
//! loses focus. While no assistive technology listens, all three return immediately (one atomic load).
//!
//! Actions are applied one at a time: once the app accepted one, the next waits until the window was drawn and a
//! fresh tree published, so every request is validated against what the AT can actually see. The reply for an
//! accepted action is sent after that fresh tree reached the bridge, so a client that reads right after its call
//! returned sees the result.

use super::action::{validate, ActionError, Request};
use super::bridge::{Bridge, Config, Mode};
use super::tree::{stamp, Ids, Tree, TreeBuilder, WindowInfo};
use crate::window_app::{Response, Waker, WindowApp};
use std::collections::VecDeque;
use std::sync::Arc;

pub struct Host {
    bridge: Option<Bridge>,
    name: String,
    ids: Ids,
    current: Option<Tree>,
    published_gen: u64,
    window: WindowInfo,
    /// Requests that arrived while an accepted action still waits for its repaint.
    queued: VecDeque<Request>,
    /// Dispatched actions whose reply goes out after the next published tree (a refusal too: its error notice
    /// is part of what the AT should find when it reads again).
    awaiting: Vec<(u64, Result<(), ActionError>)>,
    waker: Option<Waker>,
}

impl Host {
    /// A host that never publishes anything (apps without an accessibility adapter, or `DECEBAL_A11Y=0`).
    pub fn inert() -> Host {
        Host { bridge: None, name: String::new(), ids: Ids::default(), current: None, published_gen: 0, window: WindowInfo::default(), queued: VecDeque::new(), awaiting: Vec::new(), waker: None }
    }

    /// Starts the bridge worker for `app` when it has an accessibility adapter. Never blocks: the worker connects
    /// (or waits for accessibility to be enabled) on its own thread.
    pub fn start<A: WindowApp + ?Sized>(app: &A, waker: Waker) -> Host {
        let Some(name) = app.a11y_name() else { return Host::inert() };
        let cfg = Config::from_env(&name);
        if cfg.mode == Mode::Off {
            return Host::inert();
        }
        Self::with_config(cfg, waker)
    }

    pub fn with_config(cfg: Config, waker: Waker) -> Host {
        let name = cfg.app_name.clone();
        match Bridge::start(cfg, waker.clone()) {
            Ok(b) => {
                let mut h = Host::inert();
                h.bridge = Some(b);
                h.name = name;
                h.waker = Some(waker);
                h
            }
            Err(e) => {
                crate::log_warn!("accessibility bridge did not start: {e}");
                Host::inert()
            }
        }
    }

    /// An AT listens (the app should keep its accessibility state current).
    pub fn active(&self) -> bool {
        self.bridge.as_ref().is_some_and(Bridge::wants_tree)
    }

    pub fn bridge(&self) -> Option<&Bridge> {
        self.bridge.as_ref()
    }

    /// The last tree built (tests and diagnostics).
    pub fn tree(&self) -> Option<&Tree> {
        self.current.as_ref()
    }

    /// The backend reported the window's screen position (X11).
    pub fn knows_origin(&self) -> bool {
        self.window.origin.is_some()
    }

    /// Window facts changed: `origin` in root coordinates (X11; `None` keeps the last value), keyboard focus.
    pub fn set_window(&mut self, origin: Option<(i32, i32)>, active: Option<bool>) {
        let mut w = self.window;
        if origin.is_some() {
            w.origin = origin;
        }
        if let Some(a) = active {
            w.active = a;
        }
        if w == self.window {
            return;
        }
        self.window = w;
        if !self.active() {
            return;
        }
        if let Some(prev) = &self.current {
            let mut t = prev.clone();
            t.set_window(w);
            self.commit(t);
        }
    }

    fn build<A: WindowApp + ?Sized>(&mut self, app: &mut A) -> Tree {
        let mut b = TreeBuilder::new(&mut self.ids, &self.name);
        app.a11y_tree(&mut b);
        let mut t = b.finish();
        t.set_window(self.window);
        t
    }

    /// Stamps `t` against the previous tree and publishes it when anything changed (or a new registration needs it).
    fn commit(&mut self, mut t: Tree) {
        let Some(bridge) = &self.bridge else { return };
        let generation = bridge.generation();
        let changed = stamp(self.current.as_ref(), &mut t);
        if changed || generation != self.published_gen {
            self.published_gen = generation;
            bridge.publish(Arc::new(t.clone()));
        }
        self.current = Some(t);
        for (id, result) in std::mem::take(&mut self.awaiting) {
            bridge.reply(id, result);
        }
    }

    /// After a paint: describe what was drawn, publish it, answer accepted actions, and continue queued ones.
    pub fn after_draw<A: WindowApp + ?Sized>(&mut self, app: &mut A) {
        if !self.active() {
            self.fail_all();
            return;
        }
        let t = self.build(app);
        self.commit(t);
        if !self.queued.is_empty() {
            if let Some(w) = &self.waker {
                w();
            }
        }
    }

    fn fail_all(&mut self) {
        let Some(bridge) = &self.bridge else { return };
        for (id, _) in self.awaiting.drain(..) {
            bridge.reply(id, Err(ActionError::Gone));
        }
        for r in self.queued.drain(..) {
            bridge.reply(r.id, Err(ActionError::Gone));
        }
    }

    /// The event loop was woken: validate and dispatch pending requests through the app's semantic dispatcher.
    pub fn service<A: WindowApp + ?Sized>(&mut self, app: &mut A) -> Response {
        let mut resp = Response::NONE;
        let Some(bridge) = &self.bridge else { return resp };
        if !bridge.wants_tree() {
            // Nobody listens any more; anything still queued cannot be answered meaningfully.
            while let Some(r) = bridge.next_request() {
                bridge.reply(r.id, Err(ActionError::Gone));
            }
            self.fail_all();
            return resp;
        }
        if self.current.is_none() || bridge.generation() != self.published_gen {
            // A (re)registration: publish what the window shows now, without waiting for a paint.
            let t = self.build(app);
            self.commit(t);
        }
        while let Some(r) = self.bridge.as_ref().and_then(Bridge::next_request) {
            self.queued.push_back(r);
        }
        while self.awaiting.is_empty() {
            let Some(req) = self.queued.pop_front() else { break };
            let Some(tree) = &self.current else { break };
            let Some(bridge) = &self.bridge else { break };
            if req.expires.is_some_and(|t| t <= std::time::Instant::now()) {
                // The bridge already told the AT it timed out; applying it now would surprise the user.
                bridge.reply(req.id, Err(ActionError::Gone));
                continue;
            }
            let result = validate(tree, &req).and_then(|target| app.a11y_action(&target));
            match result {
                Ok(r) => {
                    resp.redraw = true;
                    if r.action.is_some() {
                        resp.action = r.action;
                    }
                    self.awaiting.push((req.id, Ok(())));
                }
                Err(e @ ActionError::Refused(_)) => {
                    resp.redraw = true;
                    self.awaiting.push((req.id, Err(e)));
                }
                Err(e) => bridge.reply(req.id, Err(e)),
            }
        }
        resp
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.fail_all();
        // Dropping the bridge stops and joins its worker.
        self.bridge = None;
    }
}
