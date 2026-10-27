//! Wayland tracker: wlr-foreign-toplevel-management for windows (Hyprland, sway, niri, river, rdwm, ...)
//! and ext-workspace-v1 for workspaces/tags (optional: only when the compositor offers it).

use super::{Control, Notify, Tracker, Window, Workspace};
use smithay_client_toolkit::reexports::client::{
    event_created_child,
    globals::{registry_queue_init, GlobalListContents},
    protocol::{wl_registry, wl_seat},
    Connection, Dispatch, Proxy, QueueHandle,
};
use smithay_client_toolkit::reexports::protocols_wlr::foreign_toplevel::v1::client::{
    zwlr_foreign_toplevel_handle_v1::{self, ZwlrForeignToplevelHandleV1},
    zwlr_foreign_toplevel_manager_v1::{self, ZwlrForeignToplevelManagerV1},
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use wayland_protocols::ext::workspace::v1::client::{
    ext_workspace_group_handle_v1::{self, ExtWorkspaceGroupHandleV1},
    ext_workspace_handle_v1::{self, ExtWorkspaceHandleV1},
    ext_workspace_manager_v1::{self, ExtWorkspaceManagerV1},
};

#[derive(Default, Clone)]
struct Pending {
    app_id: String,
    title: String,
    active: bool,
}

#[derive(Default, Clone)]
struct WsPending {
    name: String,
    active: bool,
    group: Option<u32>,
}

/// Proxies the UI thread needs to act on the compositor.
#[derive(Default)]
struct Handles {
    toplevels: HashMap<u32, ZwlrForeignToplevelHandleV1>,
    /// Workspaces in the order they are shown.
    workspaces: Vec<ExtWorkspaceHandleV1>,
    manager: Option<ExtWorkspaceManagerV1>,
    seat: Option<wl_seat::WlSeat>,
}

struct Remote {
    conn: Connection,
    handles: Arc<Mutex<Handles>>,
}

impl Control for Remote {
    fn activate(&self, id: u64) {
        let h = self.handles.lock().unwrap_or_else(|p| p.into_inner());
        if let (Some(t), Some(seat)) = (h.toplevels.get(&(id as u32)), &h.seat) {
            t.activate(seat);
        }
        drop(h);
        let _ = self.conn.flush();
    }

    fn minimize(&self, id: u64) {
        let h = self.handles.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(t) = h.toplevels.get(&(id as u32)) {
            t.set_minimized();
        }
        drop(h);
        let _ = self.conn.flush();
    }

    fn close(&self, id: u64) {
        let h = self.handles.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(t) = h.toplevels.get(&(id as u32)) {
            t.close();
        }
        drop(h);
        let _ = self.conn.flush();
    }

    fn switch_workspace(&self, index: usize) {
        let h = self.handles.lock().unwrap_or_else(|p| p.into_inner());
        if let (Some(ws), Some(m)) = (h.workspaces.get(index), &h.manager) {
            ws.activate();
            m.commit();
        }
        drop(h);
        let _ = self.conn.flush();
    }
}

struct Wm {
    tracker: Tracker,
    notify: Notify,
    toplevels: HashMap<u32, Pending>,
    workspaces: HashMap<u32, WsPending>,
    handles: Arc<Mutex<Handles>>,
}

/// The workspaces of the first output group that has any, in protocol order.
fn visible_workspaces(all: &HashMap<u32, WsPending>) -> Vec<(u32, Workspace)> {
    let mut ids: Vec<u32> = all.keys().copied().collect();
    ids.sort_unstable();
    let group = ids.iter().filter_map(|id| all[id].group).min();
    ids.into_iter().filter(|id| all[id].group == group).map(|id| (id, Workspace { name: all[&id].name.clone(), active: all[&id].active, occupied: false })).collect()
}

impl Wm {
    fn publish(&self) {
        let mut ids: Vec<_> = self.toplevels.keys().copied().collect();
        ids.sort_unstable();
        let windows = ids
            .iter()
            .map(|id| {
                let p = &self.toplevels[id];
                Window { id: *id as u64, app_id: p.app_id.clone(), title: p.title.clone(), active: p.active, desktop: None }
            })
            .collect();
        let visible = visible_workspaces(&self.workspaces);
        // A single workspace is not worth an indicator.
        let spaces: Vec<Workspace> = if visible.len() > 1 { visible.iter().map(|(_, w)| w.clone()).collect() } else { Vec::new() };
        if let Ok(mut h) = self.handles.lock() {
            let by_id = |id: u32| h.workspaces.iter().find(|w| w.id().protocol_id() == id).cloned();
            let order: Vec<ExtWorkspaceHandleV1> = visible.iter().filter_map(|(id, _)| by_id(*id)).collect();
            h.workspaces = order;
        }
        if self.tracker.replace(windows, spaces) {
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
    if globals.bind::<ZwlrForeignToplevelManagerV1, _, _>(&qh, 1..=3, ()).is_err() {
        return false;
    }
    let handles = Arc::new(Mutex::new(Handles::default()));
    if let Ok(mut h) = handles.lock() {
        h.seat = globals.bind::<wl_seat::WlSeat, _, _>(&qh, 1..=1, ()).ok();
        h.manager = globals.bind::<ExtWorkspaceManagerV1, _, _>(&qh, 1..=1, ()).ok();
    }
    tracker.set_control(Arc::new(Remote { conn: conn.clone(), handles: handles.clone() }));
    std::thread::spawn(move || {
        let mut wm = Wm { tracker, notify, toplevels: HashMap::new(), workspaces: HashMap::new(), handles };
        while queue.blocking_dispatch(&mut wm).is_ok() {}
    });
    true
}

impl Dispatch<wl_seat::WlSeat, ()> for Wm {
    fn event(_: &mut Self, _: &wl_seat::WlSeat, _: wl_seat::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<ExtWorkspaceManagerV1, ()> for Wm {
    fn event(state: &mut Self, _: &ExtWorkspaceManagerV1, event: ext_workspace_manager_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        match event {
            ext_workspace_manager_v1::Event::Workspace { workspace } => {
                let id = workspace.id().protocol_id();
                state.workspaces.insert(id, WsPending::default());
                if let Ok(mut h) = state.handles.lock() {
                    h.workspaces.push(workspace);
                }
            }
            ext_workspace_manager_v1::Event::Done => state.publish(),
            _ => {}
        }
    }

    event_created_child!(Wm, ExtWorkspaceManagerV1, [
        ext_workspace_manager_v1::EVT_WORKSPACE_GROUP_OPCODE => (ExtWorkspaceGroupHandleV1, ()),
        ext_workspace_manager_v1::EVT_WORKSPACE_OPCODE => (ExtWorkspaceHandleV1, ()),
    ]);
}

impl Dispatch<ExtWorkspaceGroupHandleV1, ()> for Wm {
    fn event(state: &mut Self, group: &ExtWorkspaceGroupHandleV1, event: ext_workspace_group_handle_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        match event {
            ext_workspace_group_handle_v1::Event::WorkspaceEnter { workspace } => {
                state.workspaces.entry(workspace.id().protocol_id()).or_default().group = Some(group.id().protocol_id());
            }
            ext_workspace_group_handle_v1::Event::WorkspaceLeave { workspace } => {
                state.workspaces.entry(workspace.id().protocol_id()).or_default().group = None;
            }
            _ => {}
        }
    }
}

impl Dispatch<ExtWorkspaceHandleV1, ()> for Wm {
    fn event(state: &mut Self, ws: &ExtWorkspaceHandleV1, event: ext_workspace_handle_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        let id = ws.id().protocol_id();
        match event {
            ext_workspace_handle_v1::Event::Name { name } => state.workspaces.entry(id).or_default().name = name,
            ext_workspace_handle_v1::Event::State { state: st } => {
                let active = matches!(st, smithay_client_toolkit::reexports::client::WEnum::Value(v) if v.contains(ext_workspace_handle_v1::State::Active));
                state.workspaces.entry(id).or_default().active = active;
            }
            ext_workspace_handle_v1::Event::Removed => {
                state.workspaces.remove(&id);
                if let Ok(mut h) = state.handles.lock() {
                    h.workspaces.retain(|w| w.id().protocol_id() != id);
                }
                ws.destroy();
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Wm {
    fn event(_: &mut Self, _: &wl_registry::WlRegistry, _: wl_registry::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for Wm {
    fn event(state: &mut Self, _: &ZwlrForeignToplevelManagerV1, event: zwlr_foreign_toplevel_manager_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let zwlr_foreign_toplevel_manager_v1::Event::Toplevel { toplevel } = event {
            let id = toplevel.id().protocol_id();
            state.toplevels.insert(id, Pending::default());
            if let Ok(mut h) = state.handles.lock() {
                h.toplevels.insert(id, toplevel);
            }
        }
    }

    event_created_child!(Wm, ZwlrForeignToplevelManagerV1, [
        zwlr_foreign_toplevel_manager_v1::EVT_TOPLEVEL_OPCODE => (ZwlrForeignToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ZwlrForeignToplevelHandleV1, ()> for Wm {
    fn event(state: &mut Self, handle: &ZwlrForeignToplevelHandleV1, event: zwlr_foreign_toplevel_handle_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
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
                let active = raw.chunks_exact(4).map(|c| u32::from_ne_bytes([c[0], c[1], c[2], c[3]])).any(|v| v == zwlr_foreign_toplevel_handle_v1::State::Activated as u32);
                state.toplevels.entry(id).or_default().active = active;
            }
            Event::Done => state.publish(),
            Event::Closed => {
                state.toplevels.remove(&id);
                if let Ok(mut h) = state.handles.lock() {
                    h.toplevels.remove(&id);
                }
                handle.destroy();
                state.publish();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws(name: &str, active: bool, group: Option<u32>) -> WsPending {
        WsPending { name: name.into(), active, group }
    }

    #[test]
    fn only_the_first_group_is_shown_in_protocol_order() {
        let all: HashMap<u32, WsPending> = [(12, ws("2", true, Some(3))), (11, ws("1", false, Some(3))), (20, ws("1", false, Some(4))), (13, ws("3", false, Some(3)))].into_iter().collect();
        let v = visible_workspaces(&all);
        assert_eq!(v.iter().map(|(id, w)| (*id, w.name.as_str(), w.active)).collect::<Vec<_>>(), vec![(11, "1", false), (12, "2", true), (13, "3", false)]);
    }

    #[test]
    fn workspaces_without_a_group_are_ignored_once_a_group_exists() {
        let all: HashMap<u32, WsPending> = [(1, ws("a", false, None)), (2, ws("b", true, Some(9)))].into_iter().collect();
        assert_eq!(visible_workspaces(&all).len(), 1);
        assert!(visible_workspaces(&HashMap::new()).is_empty());
    }
}
