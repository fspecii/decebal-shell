//! Session-bus StatusNotifier host. All protocol and icon work lives on a worker thread.
use crate::dbus::{self, Conn, Message, Value, Writer};
use std::{
    collections::HashMap,
    sync::atomic::{AtomicU64, Ordering},
    sync::{
        mpsc::{self, Receiver, SyncSender},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
mod icon;

const WATCH: &str = "org.kde.StatusNotifierWatcher";
const WP: &str = "/StatusNotifierWatcher";
const ITEM: &str = "org.kde.StatusNotifierItem";
const PROP: &str = "org.freedesktop.DBus.Properties";
const MENU: &str = "com.canonical.dbusmenu";
static NEXT: AtomicU64 = AtomicU64::new(1);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Passive,
    Active,
    NeedsAttention,
}
#[derive(Clone)]
pub struct Item {
    pub key: u64,
    pub title: String,
    pub tooltip: String,
    pub status: Status,
    pub icon_name: String,
    pub icon: Option<Arc<tiny_skia::Pixmap>>,
    pub item_is_menu: bool,
    pub menu: Option<Arc<Vec<MenuItem>>>,
    pub menu_loading: bool,
    pub menu_error: Option<String>,
}
#[derive(Clone, Debug)]
pub struct MenuItem {
    pub id: i32,
    pub label: String,
    pub enabled: bool,
    pub visible: bool,
    pub separator: bool,
    pub toggle: Option<bool>,
    pub children: Vec<MenuItem>,
    pub submenu: bool,
    pub icon_name: String,
}
#[derive(Clone, Default)]
pub struct Snapshot {
    pub items: Vec<Item>,
    pub connected: bool,
    pub generation: u64,
}
#[derive(Clone, Debug)]
pub enum Action {
    Activate { key: u64, x: i32, y: i32 },
    Secondary { key: u64, x: i32, y: i32 },
    Scroll { key: u64, delta: i32, horizontal: bool },
    OpenMenu { key: u64, x: i32, y: i32 },
    OpenSubmenu { key: u64, id: i32 },
    MenuEvent { key: u64, id: i32, timestamp: u32 },
}
#[derive(Clone)]
pub struct Host {
    state: Arc<Mutex<Snapshot>>,
    tx: SyncSender<Action>,
}
impl Host {
    pub fn start(wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (tx, rx) = mpsc::sync_channel(64);
        let state = Arc::new(Mutex::new(Snapshot::default()));
        let s = state.clone();
        // Failure drops the receiver, leaving a disconnected host and existing desktop controls usable.
        let _ = std::thread::Builder::new().name("sni-host".into()).spawn(move || run(rx, s, wake));
        Self { state, tx }
    }
    pub fn snapshot(&self) -> Snapshot {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
    pub fn send(&self, action: Action) -> bool {
        self.tx.try_send(action).is_ok()
    }
}
struct Entry {
    owner: String,
    path: String,
    aliases: Vec<String>,
    menu_path: String,
    item: Item,
    ready: bool,
    failures: u8,
}
enum Pending {
    Own,
    HostOwner(String, String),
    Owner(String, String, Option<String>),
    Props(u64),
    WatchOwner,
    WatchProps,
    About(u64),
    Layout(u64),
    Ignore,
}
struct Worker {
    icons: crate::icons::IconCache,
    host_name: String,
    hosts: HashMap<String, String>,
    conn: Conn,
    entries: Vec<Entry>,
    pending: HashMap<u32, (Instant, Pending, String)>,
    own: bool,
    watcher: String,
    watcher_failures: u8,
    watcher_refresh: Option<Instant>,
    dirty: bool,
    refresh: HashMap<u64, Instant>,
}
fn retry_delay(failures: &mut u8) -> Option<Duration> {
    if *failures >= 5 {
        return None;
    }
    let delay = Duration::from_millis(250u64 << *failures);
    *failures += 1;
    Some(delay)
}
fn strings(v: &[&str]) -> Vec<u8> {
    {
        let mut w = Writer::default();
        for s in v {
            w.string(s);
        }
        w.buf
    }
}
fn un(v: &Value) -> &Value {
    if let Value::Variant(x) = v {
        un(x)
    } else {
        v
    }
}
fn dict(v: &Value) -> &[(Value, Value)] {
    if let Value::Dict(d) = un(v) {
        d
    } else {
        &[]
    }
}
fn prop<'a>(d: &'a [(Value, Value)], name: &str) -> Option<&'a Value> {
    d.iter().find(|(k, _)| k.as_str() == Some(name)).map(|(_, v)| un(v))
}
fn text(d: &[(Value, Value)], n: &str) -> String {
    prop(d, n).and_then(Value::as_str).unwrap_or("").chars().take(4096).collect()
}
fn path(p: &str) -> bool {
    p.starts_with('/') && p.len() <= 512 && (p == "/" || p[1..].split('/').all(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')))
}
fn service(s: &str) -> bool {
    s.len() <= 255 && s.contains('.') && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b))
}
fn parse_address(s: &str) -> Option<(String, String)> {
    let i = s.find('/').unwrap_or(s.len());
    let (n, p) = s.split_at(i);
    if !service(n) {
        return None;
    }
    let p = if p.is_empty() { "/StatusNotifierItem" } else { p };
    path(p).then(|| (n.into(), p.into()))
}
impl Worker {
    // Mirrors the six typed fields of Message::call plus reply correlation.
    #[allow(clippy::too_many_arguments)]
    fn rpc(&mut self, d: &str, p: &str, i: &str, m: &str, s: &str, b: Vec<u8>, kind: Pending) {
        if self.pending.len() >= 128 {
            if let Pending::Props(k) = kind {
                self.retry(k);
            }
            return;
        }
        let retry_key = if let Pending::Props(k) = &kind { Some(*k) } else { None };
        if let Ok(serial) = self.conn.send(&Message::call(d, p, i, m, s, b)) {
            let expected = if d == WATCH && !self.watcher.is_empty() { self.watcher.clone() } else { d.to_string() };
            self.pending.insert(serial, (Instant::now(), kind, expected));
        } else if let Some(key) = retry_key {
            self.retry(key);
        }
    }
    fn bus(&mut self, m: &str, s: &str, b: Vec<u8>, p: Pending) {
        self.rpc("org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", m, s, b, p)
    }
    fn register(&mut self, owner: String, path: String, alias: String) {
        if !owner.starts_with(':') || !service(&owner) {
            return;
        }
        if let Some(e) = self.entries.iter_mut().find(|e| e.owner == owner && e.path == path) {
            if !e.aliases.contains(&alias) && e.aliases.len() < 32 {
                e.aliases.push(alias);
            }
            return;
        }
        if self.entries.len() >= 32 {
            return;
        }
        let key = NEXT.fetch_add(1, Ordering::Relaxed);
        self.entries.push(Entry {
            owner: owner.clone(),
            path: path.clone(),
            aliases: vec![alias],
            menu_path: String::new(),
            ready: false,
            failures: 0,
            item: Item {
                key,
                title: String::new(),
                tooltip: String::new(),
                status: Status::Active,
                icon_name: String::new(),
                icon: None,
                item_is_menu: false,
                menu: None,
                menu_loading: false,
                menu_error: None,
            },
        });
        self.fetch(key);
        if self.own {
            self.signal("StatusNotifierItemRegistered", Some(&format!("{owner}{path}")));
        }
    }
    fn retry_watcher(&mut self) {
        if let Some(delay) = retry_delay(&mut self.watcher_failures) {
            self.watcher_refresh = Some(Instant::now() + delay);
        }
    }
    fn add_host(&mut self, name: String, owner: String) {
        if self.hosts.len() >= 32 || self.hosts.get(&name) == Some(&owner) {
            return;
        }
        self.hosts.insert(name, owner);
        self.signal("StatusNotifierHostRegistered", None);
    }
    fn retry(&mut self, key: u64) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.item.key == key) {
            if let Some(delay) = retry_delay(&mut e.failures) {
                self.refresh.insert(key, Instant::now() + delay);
            }
        }
    }
    fn fetch(&mut self, key: u64) {
        if self.pending.values().any(|(_, p, _)| matches!(p,Pending::Props(k) if *k==key)) {
            return;
        }
        if let Some(e) = self.entries.iter().find(|e| e.item.key == key) {
            let (o, p) = (e.owner.clone(), e.path.clone());
            self.rpc(&o, &p, PROP, "GetAll", "s", strings(&[ITEM]), Pending::Props(key));
        }
    }
    fn signal(&mut self, m: &str, s: Option<&str>) {
        let _ = self.conn.send(&Message::signal(WP, WATCH, m, if s.is_some() { "s" } else { "" }, s.map(|s| strings(&[s])).unwrap_or_default()));
    }
    fn layout(&mut self, key: u64) {
        if let Some(e) = self.entries.iter().find(|e| e.item.key == key) {
            let (o, p) = (e.owner.clone(), e.menu_path.clone());
            let mut w = Writer::default();
            w.u32(0);
            w.u32(8);
            w.array(4, |_| {});
            self.rpc(&o, &p, MENU, "GetLayout", "iias", w.buf, Pending::Layout(key));
        }
    }
    fn action(&mut self, a: Action) {
        let key = match a {
            Action::Activate { key, .. }
            | Action::Secondary { key, .. }
            | Action::Scroll { key, .. }
            | Action::OpenMenu { key, .. }
            | Action::OpenSubmenu { key, .. }
            | Action::MenuEvent { key, .. } => key,
        };
        let Some(idx) = self.entries.iter().position(|e| e.item.key == key) else { return };
        let e = &self.entries[idx];
        let (o, p, mp) = (e.owner.clone(), e.path.clone(), e.menu_path.clone());
        let mut w = Writer::default();
        let mut iface = ITEM;
        let mut target = p;
        let (method, sig) = match a {
            Action::Activate { x, y, .. } => {
                w.u32(x as u32);
                w.u32(y as u32);
                ("Activate", "ii")
            }
            Action::Secondary { x, y, .. } => {
                w.u32(x as u32);
                w.u32(y as u32);
                ("SecondaryActivate", "ii")
            }
            Action::Scroll { delta, horizontal, .. } => {
                w.u32(delta as u32);
                w.string(if horizontal { "horizontal" } else { "vertical" });
                ("Scroll", "is")
            }
            Action::OpenMenu { x, y, .. } => {
                if !mp.is_empty() && mp != "/" {
                    self.entries[idx].item.menu_loading = true;
                    self.entries[idx].item.menu_error = None;
                    self.dirty = true;
                    w.u32(0);
                    self.rpc(&o, &mp, MENU, "AboutToShow", "i", w.buf, Pending::About(key));
                    return;
                }
                self.entries[idx].item.menu_loading = false;
                self.entries[idx].item.menu_error = Some("This app does not export a menu; its own context menu was requested.".into());
                self.dirty = true;
                w.u32(x as u32);
                w.u32(y as u32);
                ("ContextMenu", "ii")
            }
            Action::OpenSubmenu { id, .. } => {
                if mp.is_empty() || mp == "/" || !e.item.menu.as_ref().is_some_and(|m| menu_target(m, id, true)) {
                    return;
                }
                self.entries[idx].item.menu_loading = true;
                self.entries[idx].item.menu_error = None;
                self.dirty = true;
                w.u32(id as u32);
                self.rpc(&o, &mp, MENU, "AboutToShow", "i", w.buf, Pending::About(key));
                return;
            }
            Action::MenuEvent { id, timestamp, .. } => {
                if !e.item.menu.as_ref().is_some_and(|v| menu_target(v, id, false)) {
                    return;
                }
                w.u32(id as u32);
                w.string("clicked");
                w.sig("s");
                w.string("");
                w.u32(timestamp);
                iface = MENU;
                target = mp;
                ("Event", "isvu")
            }
        };
        self.rpc(&o, &target, iface, method, sig, w.buf, Pending::Ignore);
    }
    fn reply(&mut self, m: &Message) {
        let Some((_, _, expected)) = self.pending.get(&m.reply_serial) else { return };
        if (expected.starts_with(':') || expected == "org.freedesktop.DBus") && &m.sender != expected {
            return;
        }
        let Some((_, p, _)) = self.pending.remove(&m.reply_serial) else { return };
        let a = m.args().unwrap_or_default();
        let ok = m.kind == dbus::RETURN;
        match p {
            Pending::Own => {
                self.own = ok && a.first().and_then(Value::as_u32).is_some_and(|n| n == 1 || n == 4);
                if self.own {
                    self.hosts.insert(self.host_name.clone(), self.conn.unique.clone());
                    self.signal("StatusNotifierHostRegistered", None);
                } else {
                    self.bus("GetNameOwner", "s", strings(&[WATCH]), Pending::WatchOwner);
                }
            }
            Pending::WatchOwner => {
                if let Some(owner) = a.first().and_then(Value::as_str).filter(|s| s.starts_with(':')) {
                    self.watcher = owner.into();
                    self.watcher_failures = 0;
                    self.watcher_refresh = None;
                    self.rpc(WATCH, WP, WATCH, "RegisterStatusNotifierHost", "s", strings(&[&self.host_name.clone()]), Pending::Ignore);
                    self.rpc(WATCH, WP, PROP, "GetAll", "s", strings(&[WATCH]), Pending::WatchProps);
                }
            }
            Pending::HostOwner(name, expected) => {
                if ok && a.first().and_then(Value::as_str) == Some(expected.as_str()) {
                    self.add_host(name, expected);
                }
            }
            Pending::WatchProps => {
                if let Some(Value::Array(v)) = a.first().and_then(|v| prop(dict(v), "RegisteredStatusNotifierItems")) {
                    self.watcher_failures = 0;
                    for v in v.iter().take(32) {
                        if let Some(s) = v.as_str() {
                            self.address(s)
                        }
                    }
                } else {
                    self.retry_watcher();
                }
            }
            Pending::Owner(alias, path, expected) => {
                if let Some(owner) = a.first().and_then(Value::as_str) {
                    if expected.as_deref().is_none_or(|sender| sender == owner) {
                        self.register(owner.into(), path, alias)
                    }
                }
            }
            Pending::Props(key) => {
                if ok && matches!(a.first(), Some(Value::Dict(_))) {
                    if let Some(e) = self.entries.iter_mut().find(|e| e.item.key == key) {
                        let d = a.first().map(dict).unwrap_or(&[]);
                        e.item.title = text(d, "Title");
                        if e.item.title.is_empty() {
                            e.item.title = text(d, "Id")
                        };
                        e.item.status = match text(d, "Status").as_str() {
                            "Passive" => Status::Passive,
                            "NeedsAttention" => Status::NeedsAttention,
                            _ => Status::Active,
                        };
                        let attention = e.item.status == Status::NeedsAttention;
                        e.item.icon_name = text(d, if attention { "AttentionIconName" } else { "IconName" });
                        if e.item.icon_name.is_empty() {
                            e.item.icon_name = text(d, "IconName")
                        };
                        let theme_path = text(d, "IconThemePath");
                        let base = icon::custom(&theme_path, &e.item.icon_name)
                            .or_else(|| prop(d, if attention { "AttentionIconPixmap" } else { "IconPixmap" }).and_then(pixmap))
                            .or_else(|| prop(d, "IconPixmap").and_then(pixmap))
                            .unwrap_or_else(|| Arc::new(self.icons.get(&e.item.icon_name, 32).clone()));
                        let overlay_name = text(d, "OverlayIconName");
                        let overlay = icon::custom(&theme_path, &overlay_name).or_else(|| prop(d, "OverlayIconPixmap").and_then(pixmap)).or_else(|| {
                            if overlay_name.is_empty() {
                                None
                            } else {
                                crate::icons::load_named(&overlay_name, 32).map(Arc::new)
                            }
                        });
                        e.item.icon = Some(icon::overlay(base, overlay));
                        e.item.item_is_menu = prop(d, "ItemIsMenu").and_then(Value::as_bool).unwrap_or(false);
                        e.item.tooltip = prop(d, "ToolTip")
                            .and_then(|v| if let Value::Struct(v) = v { Some(v.iter().skip(2).filter_map(Value::as_str).collect::<Vec<_>>().join("\n").chars().take(8192).collect()) } else { None })
                            .unwrap_or_default();
                        let mp = text(d, "Menu");
                        if mp != e.menu_path {
                            e.item.menu = None;
                            e.menu_path = if path(&mp) { mp } else { String::new() }
                        }
                        e.ready = true;
                        e.failures = 0;
                        self.dirty = true;
                    }
                } else {
                    self.retry(key);
                }
            }
            Pending::About(k) => self.layout(k),
            Pending::Layout(k) => {
                if let Some(e) = self.entries.iter_mut().find(|e| e.item.key == k) {
                    e.item.menu_loading = false;
                    e.item.menu_error = None;
                    if ok {
                        let mut count = 0;
                        e.item.menu = a
                            .get(1)
                            .and_then(|v| menu_node(v, 0, &mut count))
                            .filter(|root| {
                                fn unique(n: &MenuItem, seen: &mut std::collections::HashSet<i32>) -> bool {
                                    n.id >= 0 && seen.insert(n.id) && n.children.iter().all(|c| unique(c, seen))
                                }
                                unique(root, &mut std::collections::HashSet::new())
                            })
                            .map(|n| Arc::new(n.children));
                        if e.item.menu.is_none() {
                            e.item.menu_error = Some("Invalid application menu".into())
                        }
                    } else {
                        e.item.menu_error = Some("Application menu unavailable".into())
                    }
                    self.dirty = true;
                }
            }
            Pending::Ignore => {}
        }
    }
    fn address(&mut self, s: &str) {
        if let Some((name, p)) = parse_address(s) {
            if name.starts_with(':') {
                self.register(name.clone(), p, name)
            } else {
                self.bus("GetNameOwner", "s", strings(&[&name]), Pending::Owner(name, p, None));
            }
        }
    }
    fn message(&mut self, m: Message) {
        if m.kind == dbus::RETURN || m.kind == dbus::ERROR {
            self.reply(&m);
            return;
        }
        let a = m.args().unwrap_or_default();
        if m.kind == dbus::CALL && self.own && m.path == WP {
            self.call(&m, &a);
            return;
        }
        if m.kind != dbus::SIGNAL {
            return;
        }
        if m.iface == "org.freedesktop.DBus" && m.member == "NameOwnerChanged" && m.sender == "org.freedesktop.DBus" {
            let s: Vec<_> = a.iter().filter_map(Value::as_str).collect();
            if s.len() == 3 {
                let before = self.hosts.len();
                self.hosts.retain(|name, owner| !(name == s[0] && owner != s[2] || owner == s[0] && s[2].is_empty()));
                if self.own && self.hosts.len() < before {
                    self.signal("StatusNotifierHostUnregistered", None);
                }
                let old: Vec<_> = self.entries.iter().filter(|e| e.owner == s[0] || (e.aliases.iter().any(|x| x == s[0]) && e.owner != s[2])).map(|e| format!("{}{}", e.owner, e.path)).collect();
                self.entries.retain(|e| !old.contains(&format!("{}{}", e.owner, e.path)));
                self.refresh.retain(|key, _| self.entries.iter().any(|e| e.item.key == *key));
                if !old.is_empty() {
                    self.dirty = true;
                    for id in old {
                        if self.own {
                            self.signal("StatusNotifierItemUnregistered", Some(&id));
                        }
                    }
                }
                if s[0] == WATCH && !self.own {
                    self.watcher.clear();
                    self.watcher_refresh = None;
                    if s[2].is_empty() {
                        let mut w = Writer::default();
                        w.string(WATCH);
                        w.u32(4);
                        self.bus("RequestName", "su", w.buf, Pending::Own)
                    } else {
                        self.bus("GetNameOwner", "s", strings(&[WATCH]), Pending::WatchOwner)
                    }
                }
            }
            return;
        }
        if m.sender == self.watcher && m.iface == WATCH {
            if m.member == "StatusNotifierItemRegistered" {
                if let Some(s) = a.first().and_then(Value::as_str) {
                    self.address(s)
                }
            } else if m.member == "StatusNotifierItemUnregistered" {
                if let Some(s) = a.first().and_then(Value::as_str) {
                    self.entries.retain(|e| format!("{}{}", e.owner, e.path) != s && !e.aliases.iter().any(|x| format!("{}{}", x, e.path) == s));
                    self.refresh.retain(|key, _| self.entries.iter().any(|e| e.item.key == *key));
                    self.dirty = true;
                }
            }
            return;
        }
        let keys: Vec<_> =
            self.entries.iter().filter(|e| e.owner == m.sender && (e.path == m.path || e.menu_path == m.path)).map(|e| (e.item.key, e.menu_path == m.path, e.item.menu.is_some())).collect();
        for (k, is_menu, cached) in keys {
            if is_menu && m.iface == MENU && cached {
                self.layout(k)
            } else if m.iface == ITEM || m.iface == PROP {
                if let Some(e) = self.entries.iter_mut().find(|e| e.item.key == k) {
                    e.failures = 0;
                }
                self.refresh.entry(k).or_insert_with(|| Instant::now() + Duration::from_millis(40));
            }
        }
    }
    fn call(&mut self, m: &Message, a: &[Value]) {
        let mut w = Writer::default();
        let mut sig = "";
        let mut error = None;
        if m.iface == WATCH && matches!(m.member.as_str(), "RegisterStatusNotifierItem" | "RegisterStatusNotifierHost") && (m.sig != "s" || a.len() != 1) {
            let _ = self.conn.send(&Message::error_to(m, "org.freedesktop.DBus.Error.InvalidArgs", "Expected a single service string"));
            return;
        }
        if m.iface == WATCH && m.member == "RegisterStatusNotifierItem" {
            if let Some(s) = a.first().and_then(Value::as_str) {
                if s.starts_with('/') && path(s) {
                    self.register(m.sender.clone(), s.into(), m.sender.clone())
                } else if let Some((n, p)) = parse_address(s) {
                    if n == m.sender {
                        self.register(n.clone(), p, n)
                    } else {
                        self.bus("GetNameOwner", "s", strings(&[&n]), Pending::Owner(n, p, Some(m.sender.clone())));
                    }
                } else {
                    error = Some("InvalidArgs")
                }
            } else {
                error = Some("InvalidArgs")
            }
        } else if m.iface == WATCH && m.member == "RegisterStatusNotifierHost" {
            if let Some(name) = a.first().and_then(Value::as_str).filter(|n| service(n)) {
                if name == m.sender {
                    self.add_host(name.into(), m.sender.clone());
                } else if self.hosts.len() < 32 {
                    self.bus("GetNameOwner", "s", strings(&[name]), Pending::HostOwner(name.into(), m.sender.clone()));
                }
            } else {
                error = Some("InvalidArgs");
            }
        } else if m.iface == PROP && (m.member == "GetAll" || m.member == "Get") {
            if a.first().and_then(Value::as_str) != Some(WATCH) {
                error = Some("UnknownInterface")
            } else if m.member == "GetAll" {
                sig = "a{sv}";
                w.array(8, |w| {
                    for n in ["RegisteredStatusNotifierItems", "IsStatusNotifierHostRegistered", "ProtocolVersion"] {
                        w.align(8);
                        w.string(n);
                        self.property(w, n);
                    }
                })
            } else {
                sig = "v";
                let n = a.get(1).and_then(Value::as_str).unwrap_or("");
                if !self.property(&mut w, n) {
                    error = Some("UnknownProperty")
                }
            }
        } else if m.iface == "org.freedesktop.DBus.Peer" && m.member == "Ping" {
        } else if m.iface == "org.freedesktop.DBus.Introspectable" && m.member == "Introspect" {
            sig = "s";
            w.string("<node><interface name='org.kde.StatusNotifierWatcher'><method name='RegisterStatusNotifierItem'><arg type='s' direction='in'/></method><method name='RegisterStatusNotifierHost'><arg type='s' direction='in'/></method><property name='RegisteredStatusNotifierItems' type='as' access='read'/><property name='IsStatusNotifierHostRegistered' type='b' access='read'/><property name='ProtocolVersion' type='i' access='read'/><signal name='StatusNotifierItemRegistered'><arg type='s'/></signal><signal name='StatusNotifierItemUnregistered'><arg type='s'/></signal><signal name='StatusNotifierHostRegistered'/><signal name='StatusNotifierHostUnregistered'/></interface></node>")
        } else {
            error = Some("UnknownMethod")
        }
        let reply = if let Some(err) = error { Message::error_to(m, &format!("org.freedesktop.DBus.Error.{err}"), err) } else { Message::reply_to(m, sig, w.buf) };
        let _ = self.conn.send(&reply);
    }
    fn property(&self, w: &mut Writer, n: &str) -> bool {
        match n {
            "RegisteredStatusNotifierItems" => {
                w.sig("as");
                w.array(4, |w| {
                    for e in &self.entries {
                        w.string(&format!("{}{}", e.owner, e.path));
                    }
                })
            }
            "IsStatusNotifierHostRegistered" => {
                w.sig("b");
                w.u32(u32::from(!self.hosts.is_empty()))
            }
            "ProtocolVersion" => {
                w.sig("i");
                w.u32(0)
            }
            _ => return false,
        }
        true
    }
}
fn pixmap(v: &Value) -> Option<Arc<tiny_skia::Pixmap>> {
    let Value::Array(v) = un(v) else { return None };
    let mut candidates = Vec::new();
    for v in v.iter().take(64) {
        let Value::Struct(s) = un(v) else { continue };
        if s.len() != 3 {
            continue;
        }
        let (w, h) = (s[0].as_int()?, s[1].as_int()?);
        if !(1..=512).contains(&w) || !(1..=512).contains(&h) {
            continue;
        }
        let Value::Bytes(b) = un(&s[2]) else { continue };
        if b.len() != w as usize * h as usize * 4 {
            continue;
        }
        candidates.push((w, h, b));
    }
    let (w, h, b) = candidates.into_iter().min_by_key(|(w, h, _)| (w - 32).abs() + (h - 32).abs())?;
    let mut p = tiny_skia::Pixmap::new(w as u32, h as u32)?;
    for (dst, src) in p.data_mut().chunks_exact_mut(4).zip(b.chunks_exact(4)) {
        let a = u16::from(src[0]);
        dst.copy_from_slice(&[((u16::from(src[1]) * a + 127) / 255) as u8, ((u16::from(src[2]) * a + 127) / 255) as u8, ((u16::from(src[3]) * a + 127) / 255) as u8, src[0]]);
    }
    Some(Arc::new(p))
}
fn menu_target(nodes: &[MenuItem], id: i32, submenu: bool) -> bool {
    nodes.iter().any(|n| n.enabled && n.visible && !n.separator && (n.id == id && n.submenu == submenu || menu_target(&n.children, id, submenu)))
}
fn menu_node(v: &Value, depth: usize, count: &mut usize) -> Option<MenuItem> {
    if depth > 8 || *count >= 256 {
        return None;
    }
    *count += 1;
    let Value::Struct(s) = un(v) else { return None };
    if s.len() != 3 {
        return None;
    }
    let id = i32::try_from(s[0].as_int()?).ok()?;
    if !matches!(un(&s[1]), Value::Dict(_)) {
        return None;
    }
    let d = dict(&s[1]);
    let mut children = Vec::new();
    let Value::Array(v) = un(&s[2]) else { return None };
    for v in v {
        children.push(menu_node(v, depth + 1, count)?);
    }
    Some(MenuItem {
        id,
        label: text(d, "label"),
        enabled: prop(d, "enabled").and_then(Value::as_bool).unwrap_or(true),
        visible: prop(d, "visible").and_then(Value::as_bool).unwrap_or(true),
        separator: text(d, "type") == "separator",
        toggle: if text(d, "toggle-type").is_empty() { None } else { prop(d, "toggle-state").and_then(Value::as_int).map(|n| n == 1) },
        submenu: text(d, "children-display") == "submenu" || !children.is_empty(),
        children,
        icon_name: text(d, "icon-name"),
    })
}
fn run(rx: Receiver<Action>, state: Arc<Mutex<Snapshot>>, wake: Arc<dyn Fn() + Send + Sync>) {
    loop {
        let conn = match Conn::session() {
            Ok(c) => c,
            Err(_) => match rx.recv_timeout(Duration::from_secs(2)) {
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
                _ => continue,
            },
        };
        let _ = conn.set_nonblocking(true);
        let host_name = format!("org.kde.StatusNotifierHost.decebal_{}_{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed));
        let mut w = Worker {
            icons: crate::icons::IconCache::default(),
            host_name: host_name.clone(),
            hosts: HashMap::new(),
            conn,
            entries: Vec::new(),
            pending: HashMap::new(),
            own: false,
            watcher: String::new(),
            watcher_failures: 0,
            watcher_refresh: None,
            dirty: true,
            refresh: HashMap::new(),
        };
        let mut host_request = Writer::default();
        host_request.string(&host_name);
        host_request.u32(4);
        w.bus("RequestName", "su", host_request.buf, Pending::Ignore);
        for rule in [
            "type='signal',interface='org.freedesktop.DBus',member='NameOwnerChanged'",
            "type='signal',interface='org.kde.StatusNotifierWatcher'",
            "type='signal',interface='org.kde.StatusNotifierItem'",
            "type='signal',interface='org.freedesktop.DBus.Properties'",
            "type='signal',interface='com.canonical.dbusmenu'",
        ] {
            w.bus("AddMatch", "s", strings(&[rule]), Pending::Ignore)
        }
        let mut b = Writer::default();
        b.string(WATCH);
        b.u32(4);
        w.bus("RequestName", "su", b.buf, Pending::Own);
        loop {
            for _ in 0..32 {
                match rx.try_recv() {
                    Ok(a) => w.action(a),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => return,
                }
            }
            if !matches!(w.conn.fill(), Ok(true)) {
                break;
            }
            let mut failed = false;
            for _ in 0..64 {
                match w.conn.next_message() {
                    Ok(Some(m)) => w.message(m),
                    Ok(None) => break,
                    Err(_) => {
                        failed = true;
                        break;
                    }
                }
            }
            if failed {
                break;
            }
            let now = Instant::now();
            if !w.own && !w.watcher.is_empty() && w.watcher_refresh.is_some_and(|t| t <= now) {
                w.watcher_refresh = None;
                w.rpc(WATCH, WP, PROP, "GetAll", "s", strings(&[WATCH]), Pending::WatchProps);
            }
            let keys: Vec<_> = w.refresh.iter().filter(|(_, t)| **t <= now).map(|(k, _)| *k).collect();
            for k in keys {
                w.refresh.remove(&k);
                w.fetch(k)
            }
            let expired: Vec<_> = w.pending.iter().filter(|(_, (t, _, _))| now.duration_since(*t) > Duration::from_secs(5)).map(|(k, _)| *k).collect();
            for k in expired {
                let expired = w.pending.remove(&k);
                if let Some((_, Pending::WatchProps, _)) = &expired {
                    w.retry_watcher();
                }
                if let Some((_, Pending::Props(id), _)) = &expired {
                    w.retry(*id);
                }
                if let Some((_, Pending::Layout(id) | Pending::About(id), _)) = expired {
                    if let Some(e) = w.entries.iter_mut().find(|e| e.item.key == id) {
                        e.item.menu_loading = false;
                        e.item.menu_error = Some("Application menu timed out".into());
                        w.dirty = true;
                    }
                }
            }
            if w.dirty {
                let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
                s.items = w.entries.iter().filter(|e| e.ready).map(|e| e.item.clone()).collect();
                s.connected = true;
                s.generation = s.generation.wrapping_add(1);
                drop(s);
                wake();
                w.dirty = false;
            }
            let _ = crate::backend::poll_readable(&[w.conn.fd()], Some(Duration::from_millis(50)));
        }
        {
            let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
            s.items.clear();
            s.connected = false;
            s.generation = s.generation.wrapping_add(1);
        }
        wake();
        if let Err(mpsc::RecvTimeoutError::Disconnected) = rx.recv_timeout(Duration::from_secs(1)) {
            return;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn node(id: i32, children: Vec<Value>) -> Value {
        Value::Variant(Box::new(Value::Struct(vec![
            Value::I32(id),
            Value::Dict(vec![(Value::Str("label".into()), Value::Variant(Box::new(Value::Str("_Menu".into())))), (Value::Str("enabled".into()), Value::Variant(Box::new(Value::Bool(false))))]),
            Value::Array(children),
        ])))
    }
    #[test]
    fn menu_parses_typed_nested_nodes_and_defaults() {
        let mut count = 0;
        let menu = menu_node(&node(0, vec![node(42, vec![])]), 0, &mut count).unwrap();
        assert_eq!(count, 2);
        assert_eq!(menu.children[0].id, 42);
        assert!(!menu.children[0].enabled);
        assert!(menu.children[0].visible);
    }
    #[test]
    fn menu_bounds_reject_excess_nodes_and_depth() {
        let mut count = 0;
        assert!(menu_node(&node(0, (0..256).map(|i| node(i, vec![])).collect()), 0, &mut count).is_none());
        let mut tree = node(0, vec![]);
        for i in 0..10 {
            tree = node(i, vec![tree]);
        }
        assert!(menu_node(&tree, 0, &mut 0).is_none());
    }
    #[test]
    fn transient_property_retry_is_capped_and_exponential() {
        let mut failures = 0;
        let delays: Vec<_> = std::iter::from_fn(|| retry_delay(&mut failures)).map(|d| d.as_millis()).collect();
        assert_eq!(delays, vec![250, 500, 1000, 2000, 4000]);
        assert_eq!(failures, 5);
        assert!(retry_delay(&mut failures).is_none());
        failures = 0;
        assert_eq!(retry_delay(&mut failures), Some(Duration::from_millis(250)));
    }
    #[test]
    fn string_arguments_are_not_an_array() {
        let b = strings(&["iface", "name"]);
        let mut r = dbus::Reader::new(&b, true);
        assert_eq!(r.values("ss").unwrap(), vec![Value::Str("iface".into()), Value::Str("name".into())]);
        assert_eq!(r.pos, b.len());
    }
    #[test]
    fn addresses() {
        assert_eq!(parse_address("org.example.Test").unwrap().1, "/StatusNotifierItem");
        assert!(parse_address("bad/path").is_none());
        assert!(!path("/bad-path"));
    }
    #[test]
    fn argb_is_premultiplied() {
        let v = Value::Array(vec![Value::Struct(vec![Value::I32(1), Value::I32(1), Value::Bytes(vec![128, 255, 64, 0])])]);
        assert_eq!(pixmap(&v).unwrap().data(), &[128, 32, 0, 128]);
    }
    #[test]
    fn rejects_oversized_pixmap() {
        let v = Value::Array(vec![Value::Struct(vec![Value::I32(513), Value::I32(1), Value::Bytes(vec![0; 2052])])]);
        assert!(pixmap(&v).is_none());
    }
}

#[cfg(test)]
mod private_tests;
