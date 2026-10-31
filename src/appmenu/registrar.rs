//! `com.canonical.AppMenu.Registrar`: the window → (service, menu path) table this process serves
//! when, and only when, no other registrar owns the name.

use std::collections::BTreeMap;

pub const NAME: &str = "com.canonical.AppMenu.Registrar";
pub const PATH: &str = "/com/canonical/AppMenu/Registrar";
pub const IFACE: &str = "com.canonical.AppMenu.Registrar";

pub const INTROSPECT: &str = "<!DOCTYPE node PUBLIC \"-//freedesktop//DTD D-BUS Object Introspection 1.0//EN\" \"http://www.freedesktop.org/standards/dbus/1.0/introspect.dtd\">\
<node><interface name='com.canonical.AppMenu.Registrar'>\
<method name='RegisterWindow'><arg name='windowId' type='u' direction='in'/><arg name='menuObjectPath' type='o' direction='in'/></method>\
<method name='UnregisterWindow'><arg name='windowId' type='u' direction='in'/></method>\
<method name='GetMenuForWindow'><arg name='windowId' type='u' direction='in'/><arg name='service' type='s' direction='out'/><arg name='menuObjectPath' type='o' direction='out'/></method>\
<method name='GetMenus'><arg name='menus' type='a(uso)' direction='out'/></method>\
<signal name='WindowRegistered'><arg name='windowId' type='u'/><arg name='service' type='s'/><arg name='menuObjectPath' type='o'/></signal>\
<signal name='WindowUnregistered'><arg name='windowId' type='u'/></signal>\
</interface><interface name='org.freedesktop.DBus.Introspectable'><method name='Introspect'><arg name='xml' type='s' direction='out'/></method></interface>\
<interface name='org.freedesktop.DBus.Peer'><method name='Ping'/></interface></node>";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registration {
    /// The unique bus name that registered the window (`:1.42`).
    pub service: String,
    pub path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// Window id 0 or an invalid object path.
    InvalidArgs,
    /// Another connected client already registered this window.
    Taken,
    /// The table or this sender's share of it is full.
    Full,
}

impl Refusal {
    pub fn error_name(self) -> &'static str {
        match self {
            Refusal::InvalidArgs => "org.freedesktop.DBus.Error.InvalidArgs",
            Refusal::Taken => "org.freedesktop.DBus.Error.AccessDenied",
            Refusal::Full => "org.freedesktop.DBus.Error.LimitsExceeded",
        }
    }
}

/// Registrations served while this process owns the registrar name.
#[derive(Debug, Default)]
pub struct Table {
    windows: BTreeMap<u32, Registration>,
    max_total: usize,
    max_per_sender: usize,
}

impl Table {
    pub fn new(max_total: usize, max_per_sender: usize) -> Self {
        Self { windows: BTreeMap::new(), max_total, max_per_sender }
    }

    /// Registers `window` for `sender`. Re-registering by the same sender replaces its path; a
    /// window registered by another (still connected) client is refused rather than hijacked.
    /// Returns whether the table changed.
    pub fn register(&mut self, window: u32, sender: &str, path: &str) -> Result<bool, Refusal> {
        if window == 0 || !super::bus::valid_path(path) || path == "/" || !sender.starts_with(':') {
            return Err(Refusal::InvalidArgs);
        }
        match self.windows.get(&window) {
            Some(r) if r.service != sender => return Err(Refusal::Taken),
            Some(r) if r.path == path => return Ok(false),
            Some(_) => {}
            None => {
                if self.windows.len() >= self.max_total || self.windows.values().filter(|r| r.service == sender).count() >= self.max_per_sender {
                    return Err(Refusal::Full);
                }
            }
        }
        self.windows.insert(window, Registration { service: sender.into(), path: path.into() });
        Ok(true)
    }

    /// Only the client that registered a window may unregister it.
    pub fn unregister(&mut self, window: u32, sender: &str) -> bool {
        if self.windows.get(&window).is_some_and(|r| r.service == sender) {
            self.windows.remove(&window);
            return true;
        }
        false
    }

    pub fn get(&self, window: u32) -> Option<&Registration> {
        self.windows.get(&window)
    }

    /// Forgets everything `sender` registered (it left the bus); returns the windows dropped.
    pub fn drop_sender(&mut self, sender: &str) -> Vec<u32> {
        let gone: Vec<u32> = self.windows.iter().filter(|(_, r)| r.service == sender).map(|(w, _)| *w).collect();
        for w in &gone {
            self.windows.remove(w);
        }
        gone
    }

    pub fn iter(&self) -> impl Iterator<Item = (u32, &Registration)> {
        self.windows.iter().map(|(w, r)| (*w, r))
    }

    pub fn len(&self) -> usize {
        self.windows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.windows.is_empty()
    }

    pub fn clear(&mut self) {
        self.windows.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registrations_are_owned_by_their_sender() {
        let mut t = Table::new(8, 2);
        assert_eq!(t.register(0x400001, ":1.5", "/MenuBar/1"), Ok(true));
        assert_eq!(t.register(0x400001, ":1.5", "/MenuBar/1"), Ok(false), "idempotent");
        assert_eq!(t.register(0x400001, ":1.5", "/MenuBar/2"), Ok(true), "same sender may move its menu");
        assert_eq!(t.register(0x400001, ":1.9", "/Evil"), Err(Refusal::Taken));
        assert!(!t.unregister(0x400001, ":1.9"), "only the owner unregisters");
        assert_eq!(t.get(0x400001).unwrap().path, "/MenuBar/2");
        assert!(t.unregister(0x400001, ":1.5"));
        assert!(t.get(0x400001).is_none());
    }

    #[test]
    fn invalid_arguments_and_caps_are_refused() {
        let mut t = Table::new(3, 2);
        assert_eq!(t.register(0, ":1.5", "/m"), Err(Refusal::InvalidArgs));
        assert_eq!(t.register(1, ":1.5", "/"), Err(Refusal::InvalidArgs));
        assert_eq!(t.register(1, ":1.5", "no-slash"), Err(Refusal::InvalidArgs));
        assert_eq!(t.register(1, "org.example.App", "/m"), Err(Refusal::InvalidArgs), "sender is always a unique name");
        assert_eq!(t.register(1, ":1.5", "/m"), Ok(true));
        assert_eq!(t.register(2, ":1.5", "/m"), Ok(true));
        assert_eq!(t.register(3, ":1.5", "/m"), Err(Refusal::Full), "per-sender cap");
        assert_eq!(t.register(3, ":1.6", "/m"), Ok(true));
        assert_eq!(t.register(4, ":1.7", "/m"), Err(Refusal::Full), "total cap");
        assert_eq!(t.drop_sender(":1.5"), vec![1, 2]);
        assert_eq!(t.len(), 1);
        assert_eq!(t.register(4, ":1.7", "/m"), Ok(true));
    }
}
