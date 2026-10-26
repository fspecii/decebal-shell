//! wlr-foreign-toplevel-management tracker (Hyprland, sway, niri, river, ...).

use super::{Notify, Tracker, Window};
use smithay_client_toolkit::reexports::client::{
    event_created_child,
    globals::{registry_queue_init, GlobalListContents},
    protocol::wl_registry,
    Connection, Dispatch, Proxy, QueueHandle,
};
use smithay_client_toolkit::reexports::protocols_wlr::foreign_toplevel::v1::client::{
    zwlr_foreign_toplevel_handle_v1::{self, ZwlrForeignToplevelHandleV1},
    zwlr_foreign_toplevel_manager_v1::{self, ZwlrForeignToplevelManagerV1},
};
use std::collections::HashMap;

#[derive(Default, Clone)]
struct Pending {
    app_id: String,
    title: String,
    active: bool,
}

struct Wm {
    tracker: Tracker,
    notify: Notify,
    toplevels: HashMap<u32, Pending>,
}

impl Wm {
    fn publish(&self) {
        let mut ids: Vec<_> = self.toplevels.keys().copied().collect();
        ids.sort_unstable();
        let windows = ids
            .iter()
            .map(|id| {
                let p = &self.toplevels[id];
                Window {
                    app_id: p.app_id.clone(),
                    title: p.title.clone(),
                    active: p.active,
                }
            })
            .collect();
        if self.tracker.replace(windows) {
            (self.notify)();
        }
    }
}

/// Returns true when the compositor offers the protocol and the tracker thread started.
pub fn spawn(tracker: Tracker, notify: Notify) -> bool {
    let Ok(conn) = Connection::connect_to_env() else {
        return false;
    };
    let Ok((globals, mut queue)) = registry_queue_init::<Wm>(&conn) else {
        return false;
    };
    let qh = queue.handle();
    if globals
        .bind::<ZwlrForeignToplevelManagerV1, _, _>(&qh, 1..=3, ())
        .is_err()
    {
        return false;
    }
    std::thread::spawn(move || {
        let mut wm = Wm {
            tracker,
            notify,
            toplevels: HashMap::new(),
        };
        while queue.blocking_dispatch(&mut wm).is_ok() {}
    });
    true
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Wm {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for Wm {
    fn event(
        state: &mut Self,
        _: &ZwlrForeignToplevelManagerV1,
        event: zwlr_foreign_toplevel_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwlr_foreign_toplevel_manager_v1::Event::Toplevel { toplevel } = event {
            state
                .toplevels
                .insert(toplevel.id().protocol_id(), Pending::default());
        }
    }

    event_created_child!(Wm, ZwlrForeignToplevelManagerV1, [
        zwlr_foreign_toplevel_manager_v1::EVT_TOPLEVEL_OPCODE => (ZwlrForeignToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ZwlrForeignToplevelHandleV1, ()> for Wm {
    fn event(
        state: &mut Self,
        handle: &ZwlrForeignToplevelHandleV1,
        event: zwlr_foreign_toplevel_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use zwlr_foreign_toplevel_handle_v1::Event;
        let id = handle.id().protocol_id();
        match event {
            Event::AppId { app_id } => {
                state.toplevels.entry(id).or_default().app_id = app_id;
            }
            Event::Title { title } => {
                state.toplevels.entry(id).or_default().title = title;
            }
            Event::State { state: raw } => {
                let active = raw
                    .chunks_exact(4)
                    .map(|c| u32::from_ne_bytes([c[0], c[1], c[2], c[3]]))
                    .any(|v| v == zwlr_foreign_toplevel_handle_v1::State::Activated as u32);
                state.toplevels.entry(id).or_default().active = active;
            }
            Event::Done => state.publish(),
            Event::Closed => {
                state.toplevels.remove(&id);
                handle.destroy();
                state.publish();
            }
            _ => {}
        }
    }
}
