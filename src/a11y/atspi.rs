//! The AT-SPI2 wire protocol over [`crate::dbus`], following the at-spi2-core 2.62.1 interface XML
//! (`xml/Accessible.xml`, `Application.xml`, `Component.xml`, `Action.xml`, `Value.xml`, `Text.xml`,
//! `EditableText.xml`, `Selection.xml`, `Cache.xml`, `Socket.xml`, `Event.xml`; hashes in docs/accessibility.md).
//!
//! Everything here is pure: [`handle`] answers one method call from an immutable [`Tree`] or turns it into a typed
//! [`Op`] for the UI thread, and [`events`] turns two snapshots into the signals an AT needs to keep its view current.

use super::action::{ActionError, Op};
use super::tree::{Node, NodeId, Rect, Role, State, StateSet, Tree, MASK};
use crate::dbus::{Message, Reader, Value, Writer, CALL};

pub const ROOT_PATH: &str = "/org/a11y/atspi/accessible/root";
pub const NULL_PATH: &str = "/org/a11y/atspi/null";
pub const CACHE_PATH: &str = "/org/a11y/atspi/cache";
const NODE_PREFIX: &str = "/org/a11y/atspi/accessible/";

pub const ACCESSIBLE: &str = "org.a11y.atspi.Accessible";
pub const APPLICATION: &str = "org.a11y.atspi.Application";
pub const COMPONENT: &str = "org.a11y.atspi.Component";
pub const ACTION: &str = "org.a11y.atspi.Action";
pub const VALUE: &str = "org.a11y.atspi.Value";
pub const TEXT: &str = "org.a11y.atspi.Text";
pub const EDITABLE_TEXT: &str = "org.a11y.atspi.EditableText";
pub const SELECTION: &str = "org.a11y.atspi.Selection";
pub const CACHE: &str = "org.a11y.atspi.Cache";
pub const SOCKET: &str = "org.a11y.atspi.Socket";
pub const EVENT_OBJECT: &str = "org.a11y.atspi.Event.Object";
pub const EVENT_WINDOW: &str = "org.a11y.atspi.Event.Window";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
const INTROSPECTABLE: &str = "org.freedesktop.DBus.Introspectable";
const PEER: &str = "org.freedesktop.DBus.Peer";

/// Most signals one snapshot change may emit; focus and window activation always go first.
pub const MAX_EVENTS: usize = 1024;

/// What the bridge knows about its own registration.
#[derive(Debug, Clone)]
pub struct Ctx {
    /// Our unique name on the accessibility bus.
    pub unique: String,
    /// The registry's root (the desktop), our application's parent once embedded.
    pub parent: Option<(String, String)>,
    /// `Application.Id`, assigned by the registry.
    pub app_id: i32,
    pub locale: String,
    pub toolkit_version: String,
}

/// How a deferred call is answered once the UI thread accepted it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplyKind {
    /// `b true`
    Bool,
    /// No out arguments (Properties.Set, CopyText).
    Unit,
}

pub enum Outcome {
    Reply(Message),
    /// Run on the UI thread, then answer with `kind`.
    Defer {
        node: NodeId,
        op: Op,
        kind: ReplyKind,
    },
    /// The registry assigned our `Application.Id`.
    SetAppId(i32, Message),
    /// Nothing to send (a call that asked for no reply, or not a method call).
    None,
}

pub fn path_of(id: NodeId) -> String {
    if id == NodeId::ROOT {
        ROOT_PATH.to_string()
    } else {
        format!("{NODE_PREFIX}{}", id.0)
    }
}

pub fn id_of(path: &str) -> Option<NodeId> {
    if path == ROOT_PATH {
        return Some(NodeId::ROOT);
    }
    path.strip_prefix(NODE_PREFIX)?.parse::<u64>().ok().filter(|v| *v > 0).map(NodeId)
}

// ---------------------------------------------------------------- marshalling helpers

fn i32w(w: &mut Writer, v: i32) {
    w.u32(v as u32);
}
fn boolw(w: &mut Writer, v: bool) {
    w.u32(v as u32);
}
fn i16w(w: &mut Writer, v: i16) {
    w.align(2);
    w.buf.extend_from_slice(&v.to_le_bytes());
}
fn refw(w: &mut Writer, bus: &str, path: &str) {
    w.align(8);
    w.string(bus);
    w.string(path);
}
fn noderef(w: &mut Writer, ctx: &Ctx, id: NodeId) {
    refw(w, &ctx.unique, &path_of(id));
}
fn nullref(w: &mut Writer) {
    refw(w, "", NULL_PATH);
}
fn states(w: &mut Writer, s: StateSet) {
    let [a, b] = s.words();
    w.array(4, |w| {
        w.u32(a);
        w.u32(b);
    });
}
fn dict_ss(w: &mut Writer, items: &[(String, String)]) {
    w.array(8, |w| {
        for (k, v) in items {
            w.align(8);
            w.string(k);
            w.string(v);
        }
    });
}
fn empty_props(w: &mut Writer) {
    w.array(8, |_| {});
}

/// A property value: its signature and a writer for it.
struct Prop(&'static str, Box<dyn Fn(&mut Writer)>);

fn pv<F: Fn(&mut Writer) + 'static>(sig: &'static str, f: F) -> Prop {
    Prop(sig, Box::new(f))
}

fn variant(w: &mut Writer, p: &Prop) {
    w.sig(p.0);
    (p.1)(w);
}

fn err(call: &Message, name: &str, text: &str) -> Outcome {
    if call.flags & crate::dbus::NO_REPLY_EXPECTED != 0 {
        return Outcome::None;
    }
    Outcome::Reply(Message::error_to(call, name, text))
}

fn reply(call: &Message, sig: &str, f: impl FnOnce(&mut Writer)) -> Outcome {
    if call.flags & crate::dbus::NO_REPLY_EXPECTED != 0 {
        return Outcome::None;
    }
    let mut w = Writer::default();
    f(&mut w);
    Outcome::Reply(Message::reply_to(call, sig, w.buf))
}

pub fn action_error(call: &Message, e: &ActionError) -> Message {
    Message::error_to(call, e.dbus_name(), &e.to_string())
}

pub fn deferred_ok(call: &Message, kind: ReplyKind) -> Message {
    match kind {
        ReplyKind::Bool => {
            let mut w = Writer::default();
            boolw(&mut w, true);
            Message::reply_to(call, "b", w.buf)
        }
        ReplyKind::Unit => Message::reply_to(call, "", Vec::new()),
    }
}

// ---------------------------------------------------------------- node facts

pub fn interfaces(n: &Node) -> Vec<&'static str> {
    let mut v = vec![ACCESSIBLE];
    if n.id == NodeId::ROOT {
        v.push(APPLICATION);
        return v;
    }
    v.push(COMPONENT);
    if !n.actions.is_empty() {
        v.push(ACTION);
    }
    if n.value.is_some() {
        v.push(VALUE);
    }
    if let Some(t) = &n.text {
        v.push(TEXT);
        if t.editable {
            v.push(EDITABLE_TEXT);
        }
    }
    if n.selection {
        v.push(SELECTION);
    }
    v
}

/// The node's own bounds, else its nearest ancestor's (a heading inside a card), else nothing.
fn local_bounds(t: &Tree, n: &Node) -> Rect {
    let mut cur = Some(n);
    while let Some(c) = cur {
        if let Some(b) = c.bounds {
            return b;
        }
        cur = c.parent.and_then(|p| t.get(p));
    }
    (0, 0, 0, 0)
}

/// `coord_type`: 0 screen, 1 window, 2 parent. Screen = window origin + window-relative (origin (0, 0) on Wayland).
fn extents(t: &Tree, n: &Node, coord: u32) -> Rect {
    let (x, y, w, h) = local_bounds(t, n);
    match coord {
        0 => {
            let (ox, oy) = t.window.origin.unwrap_or((0, 0));
            (x + ox, y + oy, w, h)
        }
        2 => {
            let (px, py) = n.parent.and_then(|p| t.get(p)).filter(|p| p.id != NodeId::ROOT).map(|p| local_bounds(t, p)).map(|r| (r.0, r.1)).unwrap_or((0, 0));
            (x - px, y - py, w, h)
        }
        _ => (x, y, w, h),
    }
}

fn contains(r: Rect, x: i32, y: i32) -> bool {
    x >= r.0 && y >= r.1 && x < r.0 + r.2 && y < r.1 + r.3
}

fn attributes(n: &Node) -> Vec<(String, String)> {
    let mut v = vec![("toolkit".to_string(), "decebal".to_string())];
    v.extend(n.attributes.iter().cloned());
    v
}

fn props(ctx: &Ctx, t: &Tree, n: &Node, iface: &str) -> Option<Vec<(&'static str, Prop)>> {
    let u1 = || pv("u", |w| w.u32(1));
    Some(match iface {
        ACCESSIBLE => {
            let (name, desc, key, help) = (n.name.clone(), n.description.clone(), n.key.clone(), n.help.clone());
            let parent = match (n.parent, &ctx.parent) {
                (Some(p), _) => {
                    let (bus, path) = (ctx.unique.clone(), path_of(p));
                    pv("(so)", move |w| refw(w, &bus, &path))
                }
                (None, Some((bus, path))) => {
                    let (bus, path) = (bus.clone(), path.clone());
                    pv("(so)", move |w| refw(w, &bus, &path))
                }
                (None, None) => pv("(so)", nullref),
            };
            let count = n.children.len() as i32;
            let locale = ctx.locale.clone();
            vec![
                ("version", u1()),
                ("Name", pv("s", move |w| w.string(&name))),
                ("Description", pv("s", move |w| w.string(&desc))),
                ("Parent", parent),
                ("ChildCount", pv("i", move |w| i32w(w, count))),
                ("Locale", pv("s", move |w| w.string(&locale))),
                ("AccessibleId", pv("s", move |w| w.string(&key))),
                ("HelpText", pv("s", move |w| w.string(&help))),
            ]
        }
        APPLICATION if n.id == NodeId::ROOT => {
            let (v1, v2, id) = (ctx.toolkit_version.clone(), ctx.toolkit_version.clone(), ctx.app_id);
            vec![
                ("ToolkitName", pv("s", |w| w.string("Decebal"))),
                ("Version", pv("s", move |w| w.string(&v1))),
                ("ToolkitVersion", pv("s", move |w| w.string(&v2))),
                ("AtspiVersion", pv("s", |w| w.string("2.1"))),
                ("InterfaceVersion", u1()),
                ("Id", pv("i", move |w| i32w(w, id))),
            ]
        }
        COMPONENT if n.id != NodeId::ROOT => vec![("version", u1())],
        ACTION if !n.actions.is_empty() => {
            let c = n.actions.len() as i32;
            vec![("version", u1()), ("NActions", pv("i", move |w| i32w(w, c)))]
        }
        VALUE => {
            let v = n.value.clone()?;
            let (a, b, c, d, e) = (v.min, v.max, v.step, v.current, v.text);
            vec![
                ("version", u1()),
                ("MinimumValue", pv("d", move |w| w.f64(a))),
                ("MaximumValue", pv("d", move |w| w.f64(b))),
                ("MinimumIncrement", pv("d", move |w| w.f64(c))),
                ("CurrentValue", pv("d", move |w| w.f64(d))),
                ("Text", pv("s", move |w| w.string(&e))),
            ]
        }
        TEXT => {
            let x = n.text.as_ref()?;
            let (count, caret) = (x.char_count() as i32, x.caret as i32);
            vec![("version", u1()), ("CharacterCount", pv("i", move |w| i32w(w, count))), ("CaretOffset", pv("i", move |w| i32w(w, caret)))]
        }
        EDITABLE_TEXT if n.text.as_ref().is_some_and(|x| x.editable) => vec![("version", u1())],
        SELECTION if n.selection => {
            let c = selected_children(t, n).len() as i32;
            vec![("version", u1()), ("NSelectedChildren", pv("i", move |w| i32w(w, c)))]
        }
        _ => return None,
    })
}

fn selected_children(t: &Tree, n: &Node) -> Vec<NodeId> {
    n.children.iter().copied().filter(|c| t.get(*c).is_some_and(|c| c.states.contains(State::Selected))).collect()
}

// ---------------------------------------------------------------- text helpers

fn chars(n: &Node) -> Vec<char> {
    n.text.as_ref().map(|t| t.visible().chars().collect()).unwrap_or_default()
}

/// Segments of `c` for a granularity: 0 char, 1 word, 2 sentence, 3 line, 4 paragraph. Each segment is
/// `[start, end)`; together they cover the text.
fn segments(c: &[char], gran: u32) -> Vec<(usize, usize)> {
    let n = c.len();
    let mut out = Vec::new();
    match gran {
        0 => out.extend((0..n).map(|i| (i, i + 1))),
        1 => {
            // A word and the separators after it (the WORD_START convention).
            let word = |ch: char| ch.is_alphanumeric() || ch == '_';
            let mut i = 0;
            while i < n {
                let s = i;
                while i < n && word(c[i]) {
                    i += 1;
                }
                while i < n && !word(c[i]) {
                    i += 1;
                }
                if i == s {
                    i += 1;
                }
                out.push((s, i));
            }
        }
        2 => {
            let mut s = 0;
            let mut i = 0;
            while i < n {
                let end = matches!(c[i], '.' | '!' | '?' | '\n');
                i += 1;
                if end {
                    while i < n && c[i].is_whitespace() {
                        i += 1;
                    }
                    out.push((s, i));
                    s = i;
                }
            }
            if s < n {
                out.push((s, n));
            }
        }
        _ => {
            let mut s = 0;
            for (i, ch) in c.iter().enumerate().take(n) {
                if *ch == '\n' {
                    out.push((s, i + 1));
                    s = i + 1;
                }
            }
            if s < n || out.is_empty() {
                out.push((s, n));
            }
        }
    }
    out
}

fn seg_text(c: &[char], (a, b): (usize, usize)) -> String {
    c[a.min(c.len())..b.min(c.len())].iter().collect()
}

/// (string, start, end) of the segment at / before / after `offset` (`which` = 0 / -1 / 1).
fn text_around(c: &[char], offset: i32, gran: u32, which: i32) -> (String, i32, i32) {
    let segs = segments(c, gran);
    let off = offset.max(0) as usize;
    let at = segs.iter().position(|(a, b)| off >= *a && off < *b).or_else(|| (off >= c.len()).then(|| segs.len().saturating_sub(1)).filter(|_| !segs.is_empty()));
    let Some(at) = at else { return (String::new(), offset.max(0), offset.max(0)) };
    let idx = at as i64 + which as i64;
    if idx < 0 || idx as usize >= segs.len() {
        let p = if which < 0 { 0 } else { c.len() } as i32;
        return (String::new(), p, p);
    }
    let s = segs[idx as usize];
    (seg_text(c, s), s.0 as i32, s.1 as i32)
}

/// Text boundary type (0 char, 1/2 word, 3/4 sentence, 5/6 line) to a granularity.
fn boundary_gran(b: u32) -> u32 {
    match b {
        0 => 0,
        1 | 2 => 1,
        3 | 4 => 2,
        _ => 3,
    }
}

// ---------------------------------------------------------------- dispatch

fn arg_i(a: &[Value], i: usize) -> Option<i32> {
    a.get(i).and_then(Value::as_int).map(|v| v.clamp(i32::MIN as i64, i32::MAX as i64) as i32)
}
fn arg_u(a: &[Value], i: usize) -> Option<u32> {
    a.get(i).and_then(Value::as_int).map(|v| v.clamp(0, u32::MAX as i64) as u32)
}
fn arg_s(a: &[Value], i: usize) -> Option<String> {
    a.get(i).and_then(|v| v.as_str().map(str::to_string))
}

fn unknown_method(m: &Message) -> Outcome {
    err(m, "org.freedesktop.DBus.Error.UnknownMethod", &format!("no method {}.{} with signature {:?} on {}", m.iface, m.member, m.sig, m.path))
}

fn bad_args(m: &Message, what: &str) -> Outcome {
    err(m, "org.freedesktop.DBus.Error.InvalidArgs", what)
}

fn introspect(n: Option<&Node>) -> String {
    let mut x = String::from("<!DOCTYPE node PUBLIC \"-//freedesktop//DTD D-BUS Object Introspection 1.0//EN\" \"http://www.freedesktop.org/standards/dbus/1.0/introspect.dtd\">\n<node>\n");
    x.push_str(&format!("  <interface name=\"{INTROSPECTABLE}\"/>\n  <interface name=\"{PROPERTIES}\"/>\n"));
    match n {
        Some(n) => {
            for i in interfaces(n) {
                x.push_str(&format!("  <interface name=\"{i}\"/>\n"));
            }
        }
        None => x.push_str(&format!("  <interface name=\"{CACHE}\"/>\n")),
    }
    x.push_str("</node>\n");
    x
}

/// Answers one message addressed to our objects (`m.kind` must be a method call).
pub fn handle(ctx: &Ctx, t: &Tree, m: &Message) -> Outcome {
    if m.kind != CALL {
        return Outcome::None;
    }
    let args = match m.args() {
        Ok(a) => a,
        Err(e) => return bad_args(m, &format!("cannot parse arguments: {e}")),
    };
    if m.iface == PEER {
        return match m.member.as_str() {
            "Ping" => reply(m, "", |_| {}),
            "GetMachineId" => reply(m, "s", |w| w.string(std::fs::read_to_string("/etc/machine-id").unwrap_or_default().trim())),
            _ => unknown_method(m),
        };
    }
    if m.path == CACHE_PATH {
        return match (m.iface.as_str(), m.member.as_str()) {
            (CACHE | "", "GetItems") => reply(m, "a((so)(so)(so)iiassusau)", |w| cache_items(w, ctx, t)),
            (PROPERTIES, "Get") if arg_s(&args, 0).as_deref() == Some(CACHE) && arg_s(&args, 1).as_deref() == Some("version") => reply(m, "v", |w| {
                w.sig("u");
                w.u32(1)
            }),
            (PROPERTIES, "GetAll") => reply(m, "a{sv}", |w| {
                w.array(8, |w| {
                    w.align(8);
                    w.string("version");
                    w.sig("u");
                    w.u32(1);
                })
            }),
            (INTROSPECTABLE, "Introspect") => reply(m, "s", |w| w.string(&introspect(None))),
            _ => unknown_method(m),
        };
    }
    let Some(id) = id_of(&m.path) else {
        return err(m, "org.freedesktop.DBus.Error.UnknownObject", &format!("no object at {}", m.path));
    };
    let Some(n) = t.get(id) else {
        return err(m, "org.freedesktop.DBus.Error.UnknownObject", &format!("{} no longer exists (tree revision {})", m.path, t.revision));
    };
    let ifaces = interfaces(n);
    if !m.iface.is_empty() && m.iface != PROPERTIES && m.iface != INTROSPECTABLE && !ifaces.contains(&m.iface.as_str()) {
        return err(m, "org.freedesktop.DBus.Error.UnknownInterface", &format!("{} does not implement {}", m.path, m.iface));
    }
    match m.iface.as_str() {
        PROPERTIES => properties(ctx, t, n, m, &args),
        INTROSPECTABLE if m.member == "Introspect" => reply(m, "s", |w| w.string(&introspect(Some(n)))),
        ACCESSIBLE => accessible(ctx, t, n, m, &args),
        APPLICATION => match m.member.as_str() {
            "GetLocale" => reply(m, "s", |w| w.string(&ctx.locale)),
            "GetApplicationBusAddress" => reply(m, "s", |w| w.string("")),
            _ => unknown_method(m),
        },
        COMPONENT => component(ctx, t, n, m, &args),
        ACTION => action(n, m, &args),
        TEXT => text(t, n, m, &args),
        EDITABLE_TEXT => editable(n, m, &args),
        SELECTION => selection(ctx, t, n, m, &args),
        _ => unknown_method(m),
    }
}

fn properties(ctx: &Ctx, t: &Tree, n: &Node, m: &Message, args: &[Value]) -> Outcome {
    let iface = arg_s(args, 0).unwrap_or_default();
    match m.member.as_str() {
        "Get" => {
            let name = arg_s(args, 1).unwrap_or_default();
            let Some(list) = props(ctx, t, n, &iface) else { return err(m, "org.freedesktop.DBus.Error.UnknownInterface", &format!("{} does not implement {iface}", m.path)) };
            match list.iter().find(|(k, _)| *k == name) {
                Some((_, p)) => reply(m, "v", |w| variant(w, p)),
                None => err(m, "org.freedesktop.DBus.Error.UnknownProperty", &format!("{iface} has no property {name}")),
            }
        }
        "GetAll" => {
            let list = props(ctx, t, n, &iface).unwrap_or_default();
            reply(m, "a{sv}", |w| {
                w.array(8, |w| {
                    for (k, p) in &list {
                        w.align(8);
                        w.string(k);
                        variant(w, p);
                    }
                })
            })
        }
        "Set" => {
            let name = arg_s(args, 1).unwrap_or_default();
            match (iface.as_str(), name.as_str()) {
                (APPLICATION, "Id") if n.id == NodeId::ROOT => match arg_i(args, 2) {
                    Some(v) => {
                        let ok = if m.flags & crate::dbus::NO_REPLY_EXPECTED != 0 { Message::default() } else { Message::reply_to(m, "", Vec::new()) };
                        Outcome::SetAppId(v, ok)
                    }
                    None => bad_args(m, "Id must be an integer"),
                },
                (VALUE, "CurrentValue") if n.value.is_some() => match args.get(2).and_then(Value::as_f64) {
                    Some(v) => Outcome::Defer { node: n.id, op: Op::SetValue(v), kind: ReplyKind::Unit },
                    None => bad_args(m, "CurrentValue must be a number"),
                },
                _ => err(m, "org.freedesktop.DBus.Error.PropertyReadOnly", &format!("{iface}.{name} is read-only")),
            }
        }
        _ => unknown_method(m),
    }
}

fn accessible(ctx: &Ctx, t: &Tree, n: &Node, m: &Message, args: &[Value]) -> Outcome {
    match m.member.as_str() {
        "GetChildAtIndex" => {
            let i = arg_i(args, 0).unwrap_or(-1);
            match usize::try_from(i).ok().and_then(|i| n.children.get(i)) {
                Some(c) => reply(m, "(so)", |w| noderef(w, ctx, *c)),
                None => reply(m, "(so)", nullref),
            }
        }
        "GetChildren" => reply(m, "a(so)", |w| w.array(8, |w| n.children.iter().for_each(|c| noderef(w, ctx, *c)))),
        "GetIndexInParent" => reply(m, "i", |w| i32w(w, if n.id == NodeId::ROOT { -1 } else { t.index_in_parent(n.id) })),
        "GetRelationSet" => reply(m, "a(ua(so))", |w| {
            w.array(8, |w| {
                for (r, targets) in &n.relations {
                    w.align(8);
                    w.u32(r.atspi());
                    w.array(8, |w| targets.iter().filter(|x| t.get(**x).is_some()).for_each(|x| noderef(w, ctx, *x)));
                }
            })
        }),
        "GetRole" => reply(m, "u", |w| w.u32(n.role.atspi())),
        "GetRoleName" | "GetLocalizedRoleName" => reply(m, "s", |w| w.string(n.role.name())),
        "GetState" => reply(m, "au", |w| states(w, n.states)),
        "GetAttributes" => reply(m, "a{ss}", |w| dict_ss(w, &attributes(n))),
        "GetApplication" => reply(m, "(so)", |w| noderef(w, ctx, NodeId::ROOT)),
        "GetInterfaces" => reply(m, "as", |w| w.array(4, |w| interfaces(n).iter().for_each(|i| w.string(i)))),
        _ => unknown_method(m),
    }
}

fn component(ctx: &Ctx, t: &Tree, n: &Node, m: &Message, args: &[Value]) -> Outcome {
    match m.member.as_str() {
        "Contains" => {
            let (x, y, c) = (arg_i(args, 0).unwrap_or(0), arg_i(args, 1).unwrap_or(0), arg_u(args, 2).unwrap_or(0));
            reply(m, "b", |w| boolw(w, contains(extents(t, n, c), x, y)))
        }
        "GetAccessibleAtPoint" => {
            let (x, y, c) = (arg_i(args, 0).unwrap_or(0), arg_i(args, 1).unwrap_or(0), arg_u(args, 2).unwrap_or(0));
            // Deepest showing descendant whose bounds contain the point; later siblings are drawn on top.
            let mut cur = n;
            let mut found = None;
            'down: loop {
                for cid in cur.children.iter().rev() {
                    if let Some(ch) = t.get(*cid) {
                        if ch.bounds.is_some() && ch.states.contains(State::Showing) && contains(extents(t, ch, c), x, y) {
                            found = Some(ch.id);
                            cur = ch;
                            continue 'down;
                        }
                    }
                }
                break;
            }
            match found {
                Some(id) => reply(m, "(so)", |w| noderef(w, ctx, id)),
                None => reply(m, "(so)", nullref),
            }
        }
        "GetExtents" => {
            let r = extents(t, n, arg_u(args, 0).unwrap_or(0));
            reply(m, "(iiii)", |w| {
                w.align(8);
                for v in [r.0, r.1, r.2, r.3] {
                    i32w(w, v);
                }
            })
        }
        "GetPosition" => {
            let r = extents(t, n, arg_u(args, 0).unwrap_or(0));
            reply(m, "ii", |w| {
                i32w(w, r.0);
                i32w(w, r.1)
            })
        }
        "GetSize" => {
            let r = local_bounds(t, n);
            reply(m, "ii", |w| {
                i32w(w, r.2);
                i32w(w, r.3)
            })
        }
        // AtspiComponentLayer: 3 widget, 5 popup, 7 window.
        "GetLayer" => reply(m, "u", |w| {
            w.u32(match n.role {
                Role::Frame => 7,
                Role::Menu | Role::MenuItem | Role::Dialog if n.states.contains(State::Modal) || n.role != Role::Dialog => 5,
                _ => 3,
            })
        }),
        "GetMDIZOrder" => reply(m, "n", |w| i16w(w, 0)),
        "GetAlpha" => reply(m, "d", |w| w.f64(1.0)),
        "GrabFocus" => Outcome::Defer { node: n.id, op: Op::Focus, kind: ReplyKind::Bool },
        "ScrollTo" | "ScrollToPoint" => Outcome::Defer { node: n.id, op: Op::ScrollTo, kind: ReplyKind::Bool },
        // Windows are placed by the window manager / compositor; apps never move or resize from here.
        "SetExtents" | "SetPosition" | "SetSize" => reply(m, "b", |w| boolw(w, false)),
        _ => unknown_method(m),
    }
}

fn action(n: &Node, m: &Message, args: &[Value]) -> Outcome {
    let i = arg_i(args, 0).unwrap_or(-1);
    let a = usize::try_from(i).ok().and_then(|i| n.actions.get(i));
    match m.member.as_str() {
        "GetActions" => reply(m, "a(sss)", |w| {
            w.array(8, |w| {
                for a in &n.actions {
                    w.align(8);
                    w.string(a.name);
                    w.string(&a.description);
                    w.string(&a.key_binding);
                }
            })
        }),
        "GetName" | "GetLocalizedName" | "GetDescription" | "GetKeyBinding" => {
            let Some(a) = a else { return bad_args(m, &format!("action index {i} is out of range (0..{})", n.actions.len())) };
            let s = match m.member.as_str() {
                "GetDescription" => a.description.clone(),
                "GetKeyBinding" => a.key_binding.clone(),
                _ => a.name.to_string(),
            };
            reply(m, "s", |w| w.string(&s))
        }
        "DoAction" => match a {
            Some(a) => Outcome::Defer { node: n.id, op: Op::Action(a.name), kind: ReplyKind::Bool },
            None => bad_args(m, &format!("action index {i} is out of range (0..{})", n.actions.len())),
        },
        _ => unknown_method(m),
    }
}

fn text(t: &Tree, n: &Node, m: &Message, args: &[Value]) -> Outcome {
    let Some(x) = &n.text else { return unknown_method(m) };
    let c = chars(n);
    let len = c.len() as i32;
    let clamp = |v: i32| v.clamp(0, len);
    let r = local_bounds(t, n);
    let screen = |coord: u32| extents(t, n, coord);
    let four = |m: &Message, v: Rect| {
        reply(m, "iiii", move |w| {
            for k in [v.0, v.1, v.2, v.3] {
                i32w(w, k);
            }
        })
    };
    let sel: Vec<(usize, usize)> = x.selection.into_iter().collect();
    match m.member.as_str() {
        "GetText" => {
            let a = clamp(arg_i(args, 0).unwrap_or(0));
            let e = arg_i(args, 1).unwrap_or(-1);
            let b = if e < 0 { len } else { clamp(e) };
            let s: String = if a < b { c[a as usize..b as usize].iter().collect() } else { String::new() };
            reply(m, "s", |w| w.string(&s))
        }
        "GetCharacterAtOffset" => {
            let o = arg_i(args, 0).unwrap_or(-1);
            let ch = usize::try_from(o).ok().and_then(|o| c.get(o)).map_or(0, |ch| *ch as i32);
            reply(m, "i", |w| i32w(w, ch))
        }
        "GetStringAtOffset" => {
            let (s, a, b) = text_around(&c, arg_i(args, 0).unwrap_or(0), arg_u(args, 1).unwrap_or(0), 0);
            reply(m, "sii", |w| {
                w.string(&s);
                i32w(w, a);
                i32w(w, b)
            })
        }
        "GetTextBeforeOffset" | "GetTextAtOffset" | "GetTextAfterOffset" => {
            let which = match m.member.as_str() {
                "GetTextBeforeOffset" => -1,
                "GetTextAtOffset" => 0,
                _ => 1,
            };
            let (s, a, b) = text_around(&c, arg_i(args, 0).unwrap_or(0), boundary_gran(arg_u(args, 1).unwrap_or(0)), which);
            reply(m, "sii", |w| {
                w.string(&s);
                i32w(w, a);
                i32w(w, b)
            })
        }
        "SetCaretOffset" => match arg_i(args, 0).and_then(|o| usize::try_from(o).ok()) {
            Some(o) => Outcome::Defer { node: n.id, op: Op::SetCaret(o), kind: ReplyKind::Bool },
            None => bad_args(m, "negative caret offset"),
        },
        "GetAttributeValue" => reply(m, "s", |w| w.string("")),
        "GetAttributes" | "GetAttributeRun" => reply(m, "a{ss}ii", |w| {
            dict_ss(w, &[]);
            i32w(w, 0);
            i32w(w, len)
        }),
        "GetDefaultAttributes" | "GetDefaultAttributeSet" => reply(m, "a{ss}", |w| dict_ss(w, &[])),
        // Glyph positions are not exported: a character or range reports the field's bounds (documented limit).
        "GetCharacterExtents" => four(m, screen(arg_u(args, 1).unwrap_or(0))),
        "GetRangeExtents" => four(m, screen(arg_u(args, 2).unwrap_or(0))),
        "GetOffsetAtPoint" => {
            let (px, py, coord) = (arg_i(args, 0).unwrap_or(0), arg_i(args, 1).unwrap_or(0), arg_u(args, 2).unwrap_or(0));
            let e = screen(coord);
            let o = if contains(e, px, py) && r.2 > 0 { (((px - e.0) as f32 / e.2 as f32) * len as f32).round() as i32 } else { -1 };
            reply(m, "i", |w| i32w(w, o.min(len)))
        }
        "GetNSelections" => reply(m, "i", |w| i32w(w, sel.len() as i32)),
        "GetSelection" => {
            let i = arg_i(args, 0).unwrap_or(-1);
            let (a, b) = usize::try_from(i).ok().and_then(|i| sel.get(i)).map_or((0, 0), |(a, b)| (*a as i32, *b as i32));
            reply(m, "ii", |w| {
                i32w(w, a);
                i32w(w, b)
            })
        }
        "AddSelection" => {
            let (a, b) = (arg_i(args, 0).unwrap_or(0), arg_i(args, 1).unwrap_or(0));
            let (a, b) = (clamp(a.min(b)) as usize, clamp(a.max(b)) as usize);
            Outcome::Defer { node: n.id, op: Op::SetSelection(Some((a, b))), kind: ReplyKind::Bool }
        }
        "SetSelection" => {
            if arg_i(args, 0) != Some(0) {
                return bad_args(m, "only selection 0 exists");
            }
            let (a, b) = (arg_i(args, 1).unwrap_or(0), arg_i(args, 2).unwrap_or(0));
            let (a, b) = (clamp(a.min(b)) as usize, clamp(a.max(b)) as usize);
            Outcome::Defer { node: n.id, op: Op::SetSelection(Some((a, b))), kind: ReplyKind::Bool }
        }
        "RemoveSelection" => {
            if arg_i(args, 0) != Some(0) || sel.is_empty() {
                return reply(m, "b", |w| boolw(w, false));
            }
            Outcome::Defer { node: n.id, op: Op::SetSelection(None), kind: ReplyKind::Bool }
        }
        "GetBoundedRanges" => reply(m, "a(iisv)", |w| w.array(8, |_| {})),
        "ScrollSubstringTo" | "ScrollSubstringToPoint" => Outcome::Defer { node: n.id, op: Op::ScrollTo, kind: ReplyKind::Bool },
        _ => unknown_method(m),
    }
}

fn editable(n: &Node, m: &Message, args: &[Value]) -> Outcome {
    let off = |i: usize| arg_i(args, i).and_then(|v| usize::try_from(v).ok());
    let op = match m.member.as_str() {
        "SetTextContents" => Op::SetText(arg_s(args, 0).unwrap_or_default()),
        "InsertText" => {
            let Some(o) = off(0) else { return bad_args(m, "negative offset") };
            let mut s = arg_s(args, 1).unwrap_or_default();
            // `length` counts characters of `text` to insert (-1 or larger = all of it).
            if let Some(l) = arg_i(args, 2).filter(|l| *l >= 0) {
                s = s.chars().take(l as usize).collect();
            }
            Op::InsertText { offset: o, text: s }
        }
        "DeleteText" | "CutText" | "CopyText" => {
            let (Some(a), e) = (off(0), arg_i(args, 1).unwrap_or(-1)) else { return bad_args(m, "negative offset") };
            let len = n.text.as_ref().map_or(0, |t| t.char_count());
            let b = if e < 0 { len } else { e as usize };
            match m.member.as_str() {
                "CopyText" => return Outcome::Defer { node: n.id, op: Op::CopyText { start: a, end: b }, kind: ReplyKind::Unit },
                "CutText" => Op::DeleteText { start: a, end: b, cut: true },
                _ => Op::DeleteText { start: a, end: b, cut: false },
            }
        }
        "PasteText" => match off(0) {
            Some(o) => Op::PasteText { offset: o },
            None => return bad_args(m, "negative offset"),
        },
        _ => return unknown_method(m),
    };
    Outcome::Defer { node: n.id, op, kind: ReplyKind::Bool }
}

fn selection(ctx: &Ctx, t: &Tree, n: &Node, m: &Message, args: &[Value]) -> Outcome {
    let sel = selected_children(t, n);
    let i = arg_i(args, 0).unwrap_or(-1);
    let idx = usize::try_from(i).ok();
    match m.member.as_str() {
        "GetSelectedChild" => match idx.and_then(|i| sel.get(i)) {
            Some(c) => reply(m, "(so)", |w| noderef(w, ctx, *c)),
            None => reply(m, "(so)", nullref),
        },
        "IsChildSelected" => {
            let on = idx.and_then(|i| n.children.get(i)).is_some_and(|c| sel.contains(c));
            reply(m, "b", |w| boolw(w, on))
        }
        "SelectChild" => match idx {
            Some(i) if i < n.children.len() => Outcome::Defer { node: n.id, op: Op::SelectChild(i), kind: ReplyKind::Bool },
            _ => bad_args(m, &format!("child index {i} is out of range (0..{})", n.children.len())),
        },
        // Every Decebal selection container is single-choice and never empty: deselecting is refused (false).
        "DeselectSelectedChild" | "DeselectChild" | "SelectAll" | "ClearSelection" => reply(m, "b", |w| boolw(w, false)),
        _ => unknown_method(m),
    }
}

fn cache_items(w: &mut Writer, ctx: &Ctx, t: &Tree) {
    w.array(8, |w| {
        for n in t.nodes() {
            w.align(8);
            noderef(w, ctx, n.id);
            noderef(w, ctx, NodeId::ROOT);
            match (n.parent, &ctx.parent) {
                (Some(p), _) => noderef(w, ctx, p),
                (None, Some((b, p))) => refw(w, b, p),
                (None, None) => nullref(w),
            }
            i32w(w, if n.id == NodeId::ROOT { -1 } else { t.index_in_parent(n.id) });
            i32w(w, n.children.len() as i32);
            w.array(4, |w| interfaces(n).iter().for_each(|i| w.string(i)));
            w.string(&n.name);
            w.u32(n.role.atspi());
            w.string(&n.description);
            states(w, n.states);
        }
    });
}

// ---------------------------------------------------------------- events

fn signal(iface: &str, path: &str, member: &str, detail: &str, d1: i32, d2: i32, any: Prop) -> Message {
    let mut w = Writer::default();
    w.string(detail);
    i32w(&mut w, d1);
    i32w(&mut w, d2);
    variant(&mut w, &any);
    empty_props(&mut w);
    Message::signal(path, iface, member, "siiva{sv}", w.buf)
}

fn zero() -> Prop {
    pv("i", |w| i32w(w, 0))
}

/// States worth an `object:state-changed` event.
const EVENT_STATES: [State; 19] = [
    State::Active,
    State::Busy,
    State::Checked,
    State::Defunct,
    State::Editable,
    State::Enabled,
    State::Expanded,
    State::Focusable,
    State::Focused,
    State::Indeterminate,
    State::InvalidEntry,
    State::Modal,
    State::Pressed,
    State::ReadOnly,
    State::Selected,
    State::Sensitive,
    State::Showing,
    State::Visible,
    State::Collapsed,
];

/// The signals that bring an AT from `old` to `new`, focus first, at most [`MAX_EVENTS`].
pub fn events(ctx: &Ctx, old: &Tree, new: &Tree) -> Vec<Message> {
    let mut first: Vec<Message> = Vec::new();
    let mut rest: Vec<Message> = Vec::new();
    for n in new.nodes() {
        let path = path_of(n.id);
        let Some(o) = old.get(n.id) else {
            // A new alert or notification under a node the AT already knows: announce it.
            if matches!(n.role, Role::Alert | Role::Notification) && n.parent.is_some_and(|p| old.get(p).is_some()) {
                let text = if n.description.is_empty() { n.name.clone() } else { format!("{}. {}", n.name, n.description) };
                let polite = if n.role == Role::Alert { 2 } else { 1 };
                rest.push(signal(EVENT_OBJECT, &path, "Announcement", "", polite, 0, pv("s", move |w| w.string(&text))));
            }
            continue;
        };
        if o.name != n.name {
            let v = n.name.clone();
            rest.push(signal(EVENT_OBJECT, &path, "PropertyChange", "accessible-name", 0, 0, pv("s", move |w| w.string(&v))));
        }
        if o.description != n.description {
            let v = n.description.clone();
            rest.push(signal(EVENT_OBJECT, &path, "PropertyChange", "accessible-description", 0, 0, pv("s", move |w| w.string(&v))));
        }
        if o.role != n.role {
            let v = n.role.atspi();
            rest.push(signal(EVENT_OBJECT, &path, "PropertyChange", "accessible-role", 0, 0, pv("u", move |w| w.u32(v))));
        }
        if o.parent != n.parent {
            if let Some(p) = n.parent {
                let (bus, pp) = (ctx.unique.clone(), path_of(p));
                rest.push(signal(EVENT_OBJECT, &path, "PropertyChange", "accessible-parent", 0, 0, pv("(so)", move |w| refw(w, &bus, &pp))));
            }
        }
        if let (Some(a), Some(b)) = (&o.value, &n.value) {
            if a.current.to_bits() != b.current.to_bits() {
                let v = b.current;
                rest.push(signal(EVENT_OBJECT, &path, "PropertyChange", "accessible-value", 0, 0, pv("d", move |w| w.f64(v))));
            }
        }
        for s in EVENT_STATES {
            let (was, is) = (o.states.contains(s), n.states.contains(s));
            if was != is {
                let msg = signal(EVENT_OBJECT, &path, "StateChanged", s.event_name(), is as i32, 0, zero());
                if matches!(s, State::Focused | State::Active) {
                    first.push(msg);
                } else {
                    rest.push(msg);
                }
                if s == State::Active && n.role == Role::Frame {
                    first.push(signal(EVENT_WINDOW, &path, if is { "Activate" } else { "Deactivate" }, "", 0, 0, zero()));
                }
            }
        }
        if let (Some(a), Some(b)) = (&o.text, &n.text) {
            let (ta, tb): (Vec<char>, Vec<char>) = (a.visible().chars().collect(), b.visible().chars().collect());
            if ta != tb {
                let pre = ta.iter().zip(&tb).take_while(|(x, y)| x == y).count();
                let suf = ta[pre..].iter().rev().zip(tb[pre..].iter().rev()).take_while(|(x, y)| x == y).count();
                let gone: String = ta[pre..ta.len() - suf].iter().collect();
                let added: String = tb[pre..tb.len() - suf].iter().collect();
                if !gone.is_empty() {
                    let l = gone.chars().count() as i32;
                    rest.push(signal(EVENT_OBJECT, &path, "TextChanged", "delete", pre as i32, l, pv("s", move |w| w.string(&gone))));
                }
                if !added.is_empty() {
                    let l = added.chars().count() as i32;
                    rest.push(signal(EVENT_OBJECT, &path, "TextChanged", "insert", pre as i32, l, pv("s", move |w| w.string(&added))));
                }
            }
            if a.caret != b.caret || ta != tb {
                rest.push(signal(EVENT_OBJECT, &path, "TextCaretMoved", "", b.caret as i32, 0, zero()));
            }
            if a.selection != b.selection {
                rest.push(signal(EVENT_OBJECT, &path, "TextSelectionChanged", "", 0, 0, zero()));
            }
        }
        if o.children != n.children {
            for (i, c) in o.children.iter().enumerate().rev() {
                if !n.children.contains(c) {
                    let (bus, cp) = (ctx.unique.clone(), path_of(*c));
                    rest.push(signal(EVENT_OBJECT, &path, "ChildrenChanged", "remove", i as i32, 0, pv("(so)", move |w| refw(w, &bus, &cp))));
                }
            }
            for (i, c) in n.children.iter().enumerate() {
                if !o.children.contains(c) {
                    let (bus, cp) = (ctx.unique.clone(), path_of(*c));
                    rest.push(signal(EVENT_OBJECT, &path, "ChildrenChanged", "add", i as i32, 0, pv("(so)", move |w| refw(w, &bus, &cp))));
                }
            }
        }
        if n.selection {
            let was: Vec<NodeId> = selected_children(old, o);
            let is: Vec<NodeId> = selected_children(new, n);
            if was != is {
                rest.push(signal(EVENT_OBJECT, &path, "SelectionChanged", "", 0, 0, zero()));
            }
        }
    }
    // Objects that disappeared become defunct for anyone still holding them.
    for o in old.nodes() {
        if new.get(o.id).is_none() && o.states.contains(State::Focused) {
            first.push(signal(EVENT_OBJECT, &path_of(o.id), "StateChanged", "focused", 0, 0, zero()));
        }
    }
    // Focus gained goes last among the first group so the AT ends on the new focus.
    first.sort_by_key(|m| m.member == "StateChanged" && Reader::new(&m.body, true).values("si").ok().is_some_and(|v| v[0].as_str() == Some("focused") && v[1].as_int() == Some(1)));
    first.extend(rest);
    first.truncate(MAX_EVENTS);
    first
}

/// The `Socket.Embed` call that registers our root with the registry.
pub fn embed_call(unique: &str) -> Message {
    let mut w = Writer::default();
    refw(&mut w, unique, ROOT_PATH);
    Message::call("org.a11y.atspi.Registry", ROOT_PATH, SOCKET, "Embed", "(so)", w.buf)
}

/// `Socket.Unembed`, sent best-effort when the window goes away.
pub fn unembed_call(unique: &str) -> Message {
    let mut w = Writer::default();
    refw(&mut w, unique, ROOT_PATH);
    let mut m = Message::call("org.a11y.atspi.Registry", ROOT_PATH, SOCKET, "Unembed", "(so)", w.buf);
    m.flags |= crate::dbus::NO_REPLY_EXPECTED;
    m
}

/// Characters an AT reads from a masked field (for tests and documentation).
pub fn mask_char() -> char {
    MASK
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::a11y::tree::{stamp, ActionDef, Ids, Spec, TextContent, TextInfo, TreeBuilder, ValueInfo, WindowInfo};
    use crate::dbus::RETURN;

    fn ctx() -> Ctx {
        Ctx { unique: ":1.7".into(), parent: Some(("org.a11y.atspi.Registry".into(), ROOT_PATH.into())), app_id: 3, locale: "en_US.UTF-8".into(), toolkit_version: "0.2.0".into() }
    }

    fn tree(ids: &mut Ids, on: bool, text: &str) -> Tree {
        let mut b = TreeBuilder::new(ids, "System Settings");
        let f = b.add(NodeId::ROOT, Spec::new("frame", Role::Frame, "System Settings").bounds(Some((0, 0, 900, 600)))).unwrap();
        b.add(
            f,
            Spec::new("sw", Role::Switch, "Wi-Fi")
                .state(State::Checked, on)
                .state(State::Focusable, true)
                .state(State::Showing, true)
                .bounds(Some((10, 20, 40, 24)))
                .action(ActionDef::new("toggle", "Turn on or off")),
        )
        .unwrap();
        b.add(f, Spec::new("gap", Role::Slider, "Gap").bounds(Some((10, 60, 200, 24))).value(ValueInfo { current: 4.0, min: 0.0, max: 20.0, step: 1.0, text: "4 px".into() })).unwrap();
        b.add(f, Spec::new("name", Role::Entry, "Name").bounds(Some((10, 100, 200, 24))).text(TextInfo::plain(text, 2, None, true))).unwrap();
        b.add(
            f,
            Spec::new("pw", Role::PasswordText, "Password").bounds(Some((10, 140, 200, 24))).text(TextInfo {
                content: TextContent::Masked { chars: 5 },
                caret: 5,
                selection: None,
                editable: true,
                multi_line: false,
            }),
        )
        .unwrap();
        b.focus("sw");
        let mut t = b.finish();
        t.set_window(WindowInfo { origin: Some((100, 50)), active: true });
        t
    }

    fn call(path: &str, iface: &str, member: &str, sig: &str, body: Vec<u8>) -> Message {
        let mut m = Message::call(":1.7", path, iface, member, sig, body);
        m.serial = 5;
        m.sender = ":1.9".into();
        // Through the wire format, as the bus would deliver it.
        Message::decode(&m.encode(5)).unwrap().unwrap().0
    }

    fn get(o: Outcome) -> Vec<Value> {
        match o {
            Outcome::Reply(r) => {
                assert_eq!(r.kind, RETURN, "error {}: {:?}", r.error, r.args());
                Message::decode(&r.encode(1)).unwrap().unwrap().0.args().unwrap()
            }
            _ => panic!("not a reply"),
        }
    }

    fn u32b(v: u32) -> Vec<u8> {
        let mut w = Writer::default();
        w.u32(v);
        w.buf
    }

    #[test]
    fn accessible_and_component_answer_from_the_snapshot() {
        let mut ids = Ids::default();
        let t = tree(&mut ids, true, "hello world");
        let sw = path_of(t.by_key("sw").unwrap().id);
        assert_eq!(get(handle(&ctx(), &t, &call(&sw, ACCESSIBLE, "GetRole", "", vec![])))[0], Value::U32(130));
        let st = get(handle(&ctx(), &t, &call(&sw, ACCESSIBLE, "GetState", "", vec![])));
        let Value::Array(words) = &st[0] else { panic!() };
        let low = words[0].as_u32().unwrap();
        assert!(low & (1 << 4) != 0 && low & (1 << 12) != 0, "checked + focused");
        let ext = get(handle(&ctx(), &t, &call(&sw, COMPONENT, "GetExtents", "u", u32b(0))));
        assert_eq!(ext[0], Value::Struct(vec![Value::I32(110), Value::I32(70), Value::I32(40), Value::I32(24)]), "screen = origin + local");
        let ext = get(handle(&ctx(), &t, &call(&sw, COMPONENT, "GetExtents", "u", u32b(1))));
        assert_eq!(ext[0], Value::Struct(vec![Value::I32(10), Value::I32(20), Value::I32(40), Value::I32(24)]));
        let root = get(handle(&ctx(), &t, &call(ROOT_PATH, ACCESSIBLE, "GetChildren", "", vec![])));
        let Value::Array(kids) = &root[0] else { panic!() };
        assert_eq!(kids.len(), 1);
        let ifs = get(handle(&ctx(), &t, &call(&sw, ACCESSIBLE, "GetInterfaces", "", vec![])));
        assert_eq!(ifs[0], Value::Array(vec![Value::Str(ACCESSIBLE.into()), Value::Str(COMPONENT.into()), Value::Str(ACTION.into())]));
        let mut w = Writer::default();
        w.string(ACCESSIBLE);
        w.string("AccessibleId");
        let id = get(handle(&ctx(), &t, &call(&sw, PROPERTIES, "Get", "ss", w.buf)));
        assert_eq!(id[0], Value::Variant(Box::new(Value::Str("sw".into()))));
        // Point lookup in window coordinates.
        let mut w = Writer::default();
        w.u32(20);
        w.u32(30);
        w.u32(1);
        let hit = get(handle(&ctx(), &t, &call(&path_of(t.by_key("frame").unwrap().id), COMPONENT, "GetAccessibleAtPoint", "iiu", w.buf)));
        assert_eq!(hit[0], Value::Struct(vec![Value::Str(":1.7".into()), Value::Path(sw.clone())]));
    }

    #[test]
    fn actions_and_values_defer_to_the_ui_thread() {
        let mut ids = Ids::default();
        let t = tree(&mut ids, true, "x");
        let sw = path_of(t.by_key("sw").unwrap().id);
        let mut w = Writer::default();
        w.u32(0);
        assert!(matches!(handle(&ctx(), &t, &call(&sw, ACTION, "DoAction", "i", w.buf.clone())), Outcome::Defer { op: Op::Action("toggle"), kind: ReplyKind::Bool, .. }));
        let mut bad = Writer::default();
        bad.u32(7);
        assert!(matches!(handle(&ctx(), &t, &call(&sw, ACTION, "DoAction", "i", bad.buf)), Outcome::Reply(r) if r.error == "org.freedesktop.DBus.Error.InvalidArgs"));
        let gap = path_of(t.by_key("gap").unwrap().id);
        let mut w = Writer::default();
        w.string(VALUE);
        w.string("CurrentValue");
        w.sig("d");
        w.f64(9.0);
        assert!(matches!(handle(&ctx(), &t, &call(&gap, PROPERTIES, "Set", "ssv", w.buf)), Outcome::Defer { op: Op::SetValue(v), kind: ReplyKind::Unit, .. } if v == 9.0));
        assert!(matches!(handle(&ctx(), &t, &call(&gap, COMPONENT, "GrabFocus", "", vec![])), Outcome::Defer { op: Op::Focus, .. }));
        assert!(matches!(handle(&ctx(), &t, &call(&sw, VALUE, "GetFoo", "", vec![])), Outcome::Reply(r) if r.error == "org.freedesktop.DBus.Error.UnknownInterface"));
        assert!(matches!(handle(&ctx(), &t, &call("/org/a11y/atspi/accessible/999", ACCESSIBLE, "GetRole", "", vec![])), Outcome::Reply(r) if r.error == "org.freedesktop.DBus.Error.UnknownObject"));
        // The registry assigns our id during Embed.
        let mut w = Writer::default();
        w.string(APPLICATION);
        w.string("Id");
        w.sig("i");
        w.u32(12);
        assert!(matches!(handle(&ctx(), &t, &call(ROOT_PATH, PROPERTIES, "Set", "ssv", w.buf)), Outcome::SetAppId(12, _)));
    }

    #[test]
    fn text_reads_words_lines_and_never_reveals_masked_characters() {
        let mut ids = Ids::default();
        let t = tree(&mut ids, true, "hello brave world");
        let p = path_of(t.by_key("name").unwrap().id);
        let mut w = Writer::default();
        w.u32(7);
        w.u32(1);
        let s = get(handle(&ctx(), &t, &call(&p, TEXT, "GetStringAtOffset", "iu", w.buf)));
        assert_eq!(s, vec![Value::Str("brave ".into()), Value::I32(6), Value::I32(12)]);
        let mut w = Writer::default();
        w.u32(0);
        w.u32(u32::MAX);
        let s = get(handle(&ctx(), &t, &call(&p, TEXT, "GetText", "ii", w.buf.clone())));
        assert_eq!(s[0], Value::Str("hello brave world".into()));
        let pw = path_of(t.by_key("pw").unwrap().id);
        let s = get(handle(&ctx(), &t, &call(&pw, TEXT, "GetText", "ii", w.buf)));
        assert_eq!(s[0], Value::Str("•••••".into()));
        let mut w = Writer::default();
        w.u32(0);
        w.u32(2);
        assert!(
            matches!(handle(&ctx(), &t, &call(&pw, EDITABLE_TEXT, "CopyText", "ii", w.buf)), Outcome::Defer { op: Op::CopyText { start: 0, end: 2 }, .. }),
            "rejected by validation on the UI thread"
        );
        let mut w = Writer::default();
        w.u32(3);
        w.string("Xyz");
        w.u32(1);
        assert!(matches!(handle(&ctx(), &t, &call(&p, EDITABLE_TEXT, "InsertText", "isi", w.buf)), Outcome::Defer { op: Op::InsertText { offset: 3, text }, .. } if text == "X"));
    }

    #[test]
    fn events_describe_the_change_focus_first() {
        let mut ids = Ids::default();
        let mut a = tree(&mut ids, false, "abc");
        stamp(None, &mut a);
        let mut b = tree(&mut ids, true, "abXc");
        b.focus = b.by_key("gap").map(|n| n.id);
        let w = b.window;
        b.set_window(w);
        stamp(Some(&a), &mut b);
        let ev = events(&ctx(), &a, &b);
        let names: Vec<String> = ev
            .iter()
            .map(|m| {
                let v = m.args().unwrap();
                format!("{}:{}:{}", m.member, v[0].as_str().unwrap_or(""), v[1].as_int().unwrap_or(0))
            })
            .collect();
        assert_eq!(names[0], "StateChanged:focused:0", "{names:?}");
        assert!(names.contains(&"StateChanged:checked:1".to_string()));
        assert!(names.contains(&"TextChanged:insert:2".to_string()));
        assert!(names.iter().any(|n| n.starts_with("TextCaretMoved")));
        assert!(ev.iter().all(|m| m.sig == "siiva{sv}"));
        // Identical snapshots: nothing to say.
        assert!(events(&ctx(), &b, &b.clone()).is_empty());
    }

    #[test]
    fn cache_items_cover_every_node() {
        let mut ids = Ids::default();
        let t = tree(&mut ids, true, "x");
        let v = get(handle(&ctx(), &t, &call(CACHE_PATH, CACHE, "GetItems", "", vec![])));
        let Value::Array(items) = &v[0] else { panic!() };
        assert_eq!(items.len(), t.len());
    }

    #[test]
    fn text_segments() {
        let c: Vec<char> = "One. Two!\nThree".chars().collect();
        assert_eq!(segments(&c, 2), vec![(0, 5), (5, 10), (10, 15)]);
        assert_eq!(segments(&c, 3), vec![(0, 10), (10, 15)]);
        assert_eq!(text_around(&c, 15, 3, 0), ("Three".into(), 10, 15), "the end offset belongs to the last line");
        assert_eq!(text_around(&c, 0, 3, -1), (String::new(), 0, 0));
    }
}
