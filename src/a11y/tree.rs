//! Immutable accessibility snapshots: a bounded tree of typed nodes with stable ids and revisions.
//!
//! An app describes what it shows through a [`TreeBuilder`] (roles, names, states, values, text, actions and
//! window-relative bounds). Ids come from the app's own string keys through [`Ids`], so the same control keeps the
//! same id across redraws, scrolling and rebuilds. The host stamps every node with the revision at which its
//! semantics last changed ([`stamp`]); the bridge serves the snapshot read-only from its own thread.

use std::collections::{HashMap, HashSet};

/// Window-relative rectangle in physical pixels: x, y, width, height.
pub type Rect = (i32, i32, i32, i32);

/// Most nodes one snapshot may hold; the rest is cut (the tree says so, see [`Tree::truncated`]).
pub const MAX_NODES: usize = 4096;
/// Longest name, description or help text, in bytes (longer strings end in an ellipsis).
pub const MAX_STRING: usize = 4096;
/// Longest exported text content, in bytes.
pub const MAX_TEXT: usize = 256 * 1024;

/// Stable app-local node id. `NodeId::ROOT` is the application object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub u64);

impl NodeId {
    pub const ROOT: NodeId = NodeId(0);
}

/// Roles used by Decebal windows, numbered as `AtspiRole` in at-spi2-core 2.62 (`atspi-constants.h`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    Alert,
    Application,
    CheckBox,
    ColorChooser,
    ComboBox,
    Dialog,
    Entry,
    Filler,
    Frame,
    Grouping,
    Heading,
    Image,
    Label,
    List,
    ListBox,
    ListItem,
    Menu,
    MenuItem,
    Notification,
    PageTab,
    PageTabList,
    Panel,
    PasswordText,
    PushButton,
    RadioButton,
    ScrollPane,
    Separator,
    Slider,
    SpinButton,
    Static,
    StatusBar,
    Switch,
    Text,
    ToggleButton,
}

impl Role {
    /// The `AtspiRole` number (checked against GI Atspi 2.62 by the native test).
    pub fn atspi(self) -> u32 {
        match self {
            Role::Alert => 2,
            Role::CheckBox => 7,
            Role::ColorChooser => 9,
            Role::ComboBox => 11,
            Role::Dialog => 16,
            Role::Filler => 20,
            Role::Frame => 23,
            Role::Image => 27,
            Role::Label => 29,
            Role::List => 31,
            Role::ListItem => 32,
            Role::Menu => 33,
            Role::MenuItem => 35,
            Role::PageTab => 37,
            Role::PageTabList => 38,
            Role::Panel => 39,
            Role::PasswordText => 40,
            Role::PushButton => 43,
            Role::RadioButton => 44,
            Role::ScrollPane => 49,
            Role::Separator => 50,
            Role::Slider => 51,
            Role::SpinButton => 52,
            Role::StatusBar => 54,
            Role::Text => 61,
            Role::ToggleButton => 62,
            Role::Application => 75,
            Role::Entry => 79,
            Role::Heading => 83,
            Role::ListBox => 98,
            Role::Grouping => 99,
            Role::Notification => 101,
            Role::Static => 116,
            Role::Switch => 130,
        }
    }

    /// The non-localised role name (`atspi_role_get_name`).
    pub fn name(self) -> &'static str {
        match self {
            Role::Alert => "alert",
            Role::Application => "application",
            Role::CheckBox => "check box",
            Role::ColorChooser => "color chooser",
            Role::ComboBox => "combo box",
            Role::Dialog => "dialog",
            Role::Entry => "entry",
            Role::Filler => "filler",
            Role::Frame => "frame",
            Role::Grouping => "grouping",
            Role::Heading => "heading",
            Role::Image => "image",
            Role::Label => "label",
            Role::List => "list",
            Role::ListBox => "list box",
            Role::ListItem => "list item",
            Role::Menu => "menu",
            Role::MenuItem => "menu item",
            Role::Notification => "notification",
            Role::PageTab => "page tab",
            Role::PageTabList => "page tab list",
            Role::Panel => "panel",
            Role::PasswordText => "password text",
            Role::PushButton => "button",
            Role::RadioButton => "radio button",
            Role::ScrollPane => "scroll pane",
            Role::Separator => "separator",
            Role::Slider => "slider",
            Role::SpinButton => "spin button",
            Role::Static => "static",
            Role::StatusBar => "status bar",
            Role::Switch => "switch",
            Role::Text => "text",
            Role::ToggleButton => "toggle button",
        }
    }
}

/// One `AtspiStateType` (at-spi2-core 2.62 numbering).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum State {
    Active = 1,
    Armed = 2,
    Busy = 3,
    Checked = 4,
    Collapsed = 5,
    Defunct = 6,
    Editable = 7,
    Enabled = 8,
    Expandable = 9,
    Expanded = 10,
    Focusable = 11,
    Focused = 12,
    Horizontal = 14,
    Modal = 16,
    MultiLine = 17,
    Opaque = 19,
    Pressed = 20,
    Resizable = 21,
    Selectable = 22,
    Selected = 23,
    Sensitive = 24,
    Showing = 25,
    SingleLine = 26,
    Transient = 28,
    Vertical = 29,
    Visible = 30,
    Indeterminate = 32,
    Required = 33,
    Truncated = 34,
    InvalidEntry = 36,
    SelectableText = 38,
    IsDefault = 39,
    Checkable = 41,
    HasPopup = 42,
    ReadOnly = 43,
}

impl State {
    pub const ALL: [State; 35] = [
        State::Active,
        State::Armed,
        State::Busy,
        State::Checked,
        State::Collapsed,
        State::Defunct,
        State::Editable,
        State::Enabled,
        State::Expandable,
        State::Expanded,
        State::Focusable,
        State::Focused,
        State::Horizontal,
        State::Modal,
        State::MultiLine,
        State::Opaque,
        State::Pressed,
        State::Resizable,
        State::Selectable,
        State::Selected,
        State::Sensitive,
        State::Showing,
        State::SingleLine,
        State::Transient,
        State::Vertical,
        State::Visible,
        State::Indeterminate,
        State::Required,
        State::Truncated,
        State::InvalidEntry,
        State::SelectableText,
        State::IsDefault,
        State::Checkable,
        State::HasPopup,
        State::ReadOnly,
    ];

    /// The detail string of `object:state-changed:<name>` events.
    pub fn event_name(self) -> &'static str {
        match self {
            State::Active => "active",
            State::Armed => "armed",
            State::Busy => "busy",
            State::Checked => "checked",
            State::Collapsed => "collapsed",
            State::Defunct => "defunct",
            State::Editable => "editable",
            State::Enabled => "enabled",
            State::Expandable => "expandable",
            State::Expanded => "expanded",
            State::Focusable => "focusable",
            State::Focused => "focused",
            State::Horizontal => "horizontal",
            State::Modal => "modal",
            State::MultiLine => "multi-line",
            State::Opaque => "opaque",
            State::Pressed => "pressed",
            State::Resizable => "resizable",
            State::Selectable => "selectable",
            State::Selected => "selected",
            State::Sensitive => "sensitive",
            State::Showing => "showing",
            State::SingleLine => "single-line",
            State::Transient => "transient",
            State::Vertical => "vertical",
            State::Visible => "visible",
            State::Indeterminate => "indeterminate",
            State::Required => "required",
            State::Truncated => "truncated",
            State::InvalidEntry => "invalid-entry",
            State::SelectableText => "selectable-text",
            State::IsDefault => "is-default",
            State::Checkable => "checkable",
            State::HasPopup => "has-popup",
            State::ReadOnly => "read-only",
        }
    }
}

/// A set of [`State`]s, one bit per `AtspiStateType` number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct StateSet(pub u64);

impl StateSet {
    pub fn of(states: &[State]) -> StateSet {
        let mut s = StateSet::default();
        for &st in states {
            s.insert(st);
        }
        s
    }
    pub fn insert(&mut self, s: State) {
        self.0 |= 1 << s as u8;
    }
    pub fn remove(&mut self, s: State) {
        self.0 &= !(1 << s as u8);
    }
    pub fn set(&mut self, s: State, on: bool) {
        if on {
            self.insert(s)
        } else {
            self.remove(s)
        }
    }
    pub fn with(mut self, s: State, on: bool) -> StateSet {
        self.set(s, on);
        self
    }
    pub fn contains(self, s: State) -> bool {
        self.0 & (1 << s as u8) != 0
    }
    /// The `au` wire form of `GetState`: two 32-bit words, low bits first.
    pub fn words(self) -> [u32; 2] {
        [self.0 as u32, (self.0 >> 32) as u32]
    }
    /// States that describe what a control is and does; a change of these makes earlier actions on it stale.
    /// Focus, visibility and geometry are excluded: scrolling or tabbing must not invalidate a pending action.
    pub fn semantic(self) -> StateSet {
        let keep = StateSet::of(&[
            State::Checked,
            State::Enabled,
            State::Sensitive,
            State::Selected,
            State::Expanded,
            State::Pressed,
            State::Editable,
            State::ReadOnly,
            State::Indeterminate,
            State::Modal,
            State::Checkable,
            State::Selectable,
        ]);
        StateSet(self.0 & keep.0)
    }
}

/// The Value interface of a slider or spin button.
#[derive(Debug, Clone, PartialEq)]
pub struct ValueInfo {
    pub current: f64,
    pub min: f64,
    pub max: f64,
    /// Smallest change the control accepts (0 = continuous).
    pub step: f64,
    /// The value as the control shows it ("12 px").
    pub text: String,
}

/// What a text node exposes. Password-like fields never carry their characters, only how many bullets are shown.
#[derive(Debug, Clone, PartialEq)]
pub enum TextContent {
    Plain(String),
    Masked { chars: usize },
}

/// The bullet a masked field shows per character.
pub const MASK: char = '\u{2022}';

#[derive(Debug, Clone, PartialEq)]
pub struct TextInfo {
    pub content: TextContent,
    /// Caret, in characters.
    pub caret: usize,
    /// Selected range in characters (start < end), if any.
    pub selection: Option<(usize, usize)>,
    pub editable: bool,
    pub multi_line: bool,
}

impl TextInfo {
    pub fn plain(text: &str, caret: usize, selection: Option<(usize, usize)>, editable: bool) -> TextInfo {
        TextInfo { content: TextContent::Plain(text.to_string()), caret, selection, editable, multi_line: false }
    }
    /// The characters an AT may read: the text itself, or one bullet per masked character.
    pub fn visible(&self) -> String {
        match &self.content {
            TextContent::Plain(t) => t.clone(),
            TextContent::Masked { chars } => std::iter::repeat_n(MASK, *chars).collect(),
        }
    }
    pub fn char_count(&self) -> usize {
        match &self.content {
            TextContent::Plain(t) => t.chars().count(),
            TextContent::Masked { chars } => *chars,
        }
    }
    pub fn masked(&self) -> bool {
        matches!(self.content, TextContent::Masked { .. })
    }
}

/// One entry of the Action interface. `name` is the stable, non-localised verb the dispatcher matches on.
#[derive(Debug, Clone, PartialEq)]
pub struct ActionDef {
    pub name: &'static str,
    pub description: String,
    pub key_binding: String,
}

impl ActionDef {
    pub fn new(name: &'static str, description: impl Into<String>) -> ActionDef {
        ActionDef { name, description: description.into(), key_binding: String::new() }
    }
    pub fn key(mut self, k: impl Into<String>) -> ActionDef {
        self.key_binding = k.into();
        self
    }
}

/// `AtspiRelationType` values used here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Relation {
    LabelledBy,
    LabelFor,
    DescribedBy,
    ErrorMessage,
    PopupFor,
    ControllerFor,
}

impl Relation {
    pub fn atspi(self) -> u32 {
        match self {
            Relation::LabelFor => 1,
            Relation::LabelledBy => 2,
            Relation::ControllerFor => 3,
            Relation::PopupFor => 15,
            Relation::DescribedBy => 18,
            Relation::ErrorMessage => 21,
        }
    }
}

/// Everything an app says about one node. Children and parent are filled by the builder.
#[derive(Debug, Clone, PartialEq)]
pub struct Spec {
    pub key: String,
    pub role: Role,
    pub name: String,
    pub description: String,
    pub help: String,
    pub states: StateSet,
    pub bounds: Option<Rect>,
    pub value: Option<ValueInfo>,
    pub text: Option<TextInfo>,
    pub actions: Vec<ActionDef>,
    /// The node implements Selection over its children (lists, tab strips, combo menus, radio groups).
    pub selection: bool,
    /// AT-SPI object attributes (`level` of a heading, `placeholder-text`, ...).
    pub attributes: Vec<(String, String)>,
    /// Relations by target key (resolved when the tree is finished; unknown keys are dropped).
    pub relations: Vec<(Relation, String)>,
}

impl Spec {
    pub fn new(key: impl Into<String>, role: Role, name: impl Into<String>) -> Spec {
        Spec {
            key: key.into(),
            role,
            name: name.into(),
            description: String::new(),
            help: String::new(),
            states: StateSet::of(&[State::Enabled, State::Sensitive, State::Visible]),
            bounds: None,
            value: None,
            text: None,
            actions: Vec::new(),
            selection: false,
            attributes: Vec::new(),
            relations: Vec::new(),
        }
    }
    pub fn description(mut self, d: impl Into<String>) -> Spec {
        self.description = d.into();
        self
    }
    pub fn bounds(mut self, r: Option<Rect>) -> Spec {
        self.bounds = r;
        self
    }
    pub fn state(mut self, s: State, on: bool) -> Spec {
        self.states.set(s, on);
        self
    }
    pub fn enabled(self, on: bool) -> Spec {
        self.state(State::Enabled, on).state(State::Sensitive, on)
    }
    pub fn action(mut self, a: ActionDef) -> Spec {
        self.actions.push(a);
        self
    }
    pub fn value(mut self, v: ValueInfo) -> Spec {
        self.value = Some(v);
        self
    }
    pub fn text(mut self, t: TextInfo) -> Spec {
        self.text = Some(t);
        self
    }
    pub fn selection(mut self) -> Spec {
        self.selection = true;
        self
    }
    pub fn attr(mut self, k: &str, v: impl Into<String>) -> Spec {
        self.attributes.push((k.to_string(), v.into()));
        self
    }
    pub fn relation(mut self, r: Relation, key: impl Into<String>) -> Spec {
        self.relations.push((r, key.into()));
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub id: NodeId,
    pub key: String,
    pub role: Role,
    pub name: String,
    pub description: String,
    pub help: String,
    pub states: StateSet,
    pub bounds: Option<Rect>,
    pub value: Option<ValueInfo>,
    pub text: Option<TextInfo>,
    pub actions: Vec<ActionDef>,
    pub selection: bool,
    pub attributes: Vec<(String, String)>,
    pub relations: Vec<(Relation, Vec<NodeId>)>,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    /// Revision at which this node's semantics (not its focus or geometry) last changed; see [`stamp`].
    pub changed: u64,
}

impl Node {
    /// Same role, name, value, text, actions, children and semantic states (ignores focus, showing and bounds).
    pub fn same_semantics(&self, o: &Node) -> bool {
        self.role == o.role
            && self.name == o.name
            && self.description == o.description
            && self.states.semantic() == o.states.semantic()
            && self.value.as_ref().map(|v| (v.current.to_bits(), v.min.to_bits(), v.max.to_bits())) == o.value.as_ref().map(|v| (v.current.to_bits(), v.min.to_bits(), v.max.to_bits()))
            && self.text.as_ref().map(|t| (&t.content, t.editable)) == o.text.as_ref().map(|t| (&t.content, t.editable))
            && self.actions.iter().map(|a| a.name).eq(o.actions.iter().map(|a| a.name))
            && self.children == o.children
    }
    pub fn enabled(&self) -> bool {
        self.states.contains(State::Enabled) && self.states.contains(State::Sensitive)
    }
}

/// Window facts the host adds to a snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct WindowInfo {
    /// The window's top-left corner in root/screen coordinates when the backend knows it (X11); `None` on Wayland,
    /// where clients cannot learn their global position.
    pub origin: Option<(i32, i32)>,
    /// The window has keyboard focus.
    pub active: bool,
}

/// One immutable snapshot. Node 0 is the application; nodes are in depth-first order.
#[derive(Debug, Clone, Default)]
pub struct Tree {
    pub revision: u64,
    nodes: Vec<Node>,
    index: HashMap<NodeId, usize>,
    /// The node with keyboard focus (also carries [`State::Focused`] while the window is active).
    pub focus: Option<NodeId>,
    pub window: WindowInfo,
    /// The app described more than [`MAX_NODES`] nodes.
    pub truncated: bool,
}

impl PartialEq for Tree {
    fn eq(&self, o: &Tree) -> bool {
        self.nodes == o.nodes && self.focus == o.focus && self.window == o.window && self.truncated == o.truncated
    }
}

impl Tree {
    pub fn get(&self, id: NodeId) -> Option<&Node> {
        self.index.get(&id).map(|&i| &self.nodes[i])
    }
    pub fn root(&self) -> Option<&Node> {
        self.nodes.first()
    }
    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }
    pub fn len(&self) -> usize {
        self.nodes.len()
    }
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
    pub fn by_key(&self, key: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.key == key)
    }
    pub fn index_in_parent(&self, id: NodeId) -> i32 {
        let Some(n) = self.get(id) else { return -1 };
        let Some(p) = n.parent.and_then(|p| self.get(p)) else { return -1 };
        p.children.iter().position(|c| *c == id).map_or(-1, |i| i as i32)
    }
    /// Bounds in screen coordinates: window origin plus the window-relative rectangle (origin (0, 0) when unknown).
    pub fn screen_bounds(&self, n: &Node) -> Option<Rect> {
        let (ox, oy) = self.window.origin.unwrap_or((0, 0));
        n.bounds.map(|(x, y, w, h)| (x + ox, y + oy, w, h))
    }
    /// Applies window facts and refreshes the focus/active/showing states that depend on them.
    pub fn set_window(&mut self, w: WindowInfo) {
        self.window = w;
        let focus = self.focus;
        for n in &mut self.nodes {
            if n.role == Role::Frame || n.role == Role::Dialog && n.parent == Some(NodeId::ROOT) {
                n.states.set(State::Active, w.active);
            }
            if n.states.contains(State::Focusable) || Some(n.id) == focus {
                n.states.set(State::Focused, w.active && Some(n.id) == focus);
            }
        }
    }
}

/// Maps app keys to stable numeric ids for the life of a window. Ids are never reused.
#[derive(Debug, Default)]
pub struct Ids {
    map: HashMap<String, u64>,
}

impl Ids {
    pub fn id(&mut self, key: &str) -> NodeId {
        if let Some(&v) = self.map.get(key) {
            return NodeId(v);
        }
        let v = self.map.len() as u64 + 1;
        self.map.insert(key.to_string(), v);
        NodeId(v)
    }
    pub fn len(&self) -> usize {
        self.map.len()
    }
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

fn clip(mut s: String, max: usize) -> String {
    if s.len() <= max {
        return s;
    }
    let mut cut = max.saturating_sub(3);
    while !s.is_char_boundary(cut) {
        cut -= 1;
    }
    s.truncate(cut);
    s.push('…');
    s
}

/// Builds one snapshot. Keys must be unique; a repeated key gets a deterministic `#n` suffix.
pub struct TreeBuilder<'a> {
    ids: &'a mut Ids,
    nodes: Vec<Node>,
    pending_rel: Vec<(usize, Relation, String)>,
    seen: HashSet<String>,
    focus_key: Option<String>,
    truncated: bool,
}

impl<'a> TreeBuilder<'a> {
    /// Starts a tree whose application object is called `app_name`.
    pub fn new(ids: &'a mut Ids, app_name: &str) -> TreeBuilder<'a> {
        let root = Node {
            id: NodeId::ROOT,
            key: String::new(),
            role: Role::Application,
            name: clip(app_name.to_string(), MAX_STRING),
            description: String::new(),
            help: String::new(),
            states: StateSet::default(),
            bounds: None,
            value: None,
            text: None,
            actions: Vec::new(),
            selection: false,
            attributes: Vec::new(),
            relations: Vec::new(),
            parent: None,
            children: Vec::new(),
            changed: 0,
        };
        TreeBuilder { ids, nodes: vec![root], pending_rel: Vec::new(), seen: HashSet::new(), focus_key: None, truncated: false }
    }

    /// Adds a child of `parent` (built earlier); `None` once the node budget is spent.
    pub fn add(&mut self, parent: NodeId, spec: Spec) -> Option<NodeId> {
        if self.nodes.len() >= MAX_NODES {
            self.truncated = true;
            return None;
        }
        let mut key = spec.key;
        if !self.seen.insert(key.clone()) {
            let mut n = 2;
            while !self.seen.insert(format!("{key}#{n}")) {
                n += 1;
            }
            key = format!("{key}#{n}");
        }
        let id = self.ids.id(&key);
        let pi = self.nodes.iter().rposition(|n| n.id == parent)?;
        self.nodes[pi].children.push(id);
        let text = spec.text.map(|mut t| {
            if let TextContent::Plain(s) = &mut t.content {
                *s = clip(std::mem::take(s), MAX_TEXT);
            }
            let n = t.char_count();
            t.caret = t.caret.min(n);
            t.selection = t.selection.map(|(a, b)| (a.min(n), b.min(n))).filter(|(a, b)| a < b);
            t
        });
        let idx = self.nodes.len();
        for (r, k) in spec.relations {
            self.pending_rel.push((idx, r, k));
        }
        self.nodes.push(Node {
            id,
            key,
            role: spec.role,
            name: clip(spec.name, MAX_STRING),
            description: clip(spec.description, MAX_STRING),
            help: clip(spec.help, MAX_STRING),
            states: spec.states,
            bounds: spec.bounds.map(|(x, y, w, h)| (x, y, w.max(0), h.max(0))),
            value: spec.value.filter(|v| v.current.is_finite() && v.min.is_finite() && v.max.is_finite()),
            text,
            actions: spec.actions,
            selection: spec.selection,
            attributes: spec.attributes.into_iter().map(|(k, v)| (k, clip(v, MAX_STRING))).collect(),
            relations: Vec::new(),
            parent: Some(parent),
            children: Vec::new(),
            changed: 0,
        });
        Some(id)
    }

    /// Marks the node with `key` (added before or after this call) as the keyboard focus.
    pub fn focus(&mut self, key: &str) {
        self.focus_key = Some(key.to_string());
    }

    pub fn truncated(&self) -> bool {
        self.truncated
    }

    pub fn finish(mut self) -> Tree {
        // Depth-first order (children were appended as they were added; parents always come first).
        let mut order = Vec::with_capacity(self.nodes.len());
        let pos: HashMap<NodeId, usize> = self.nodes.iter().enumerate().map(|(i, n)| (n.id, i)).collect();
        let mut stack = vec![0usize];
        while let Some(i) = stack.pop() {
            order.push(i);
            for c in self.nodes[i].children.iter().rev() {
                if let Some(&ci) = pos.get(c) {
                    stack.push(ci);
                }
            }
        }
        let keyed: HashMap<String, NodeId> = self.nodes.iter().map(|n| (n.key.clone(), n.id)).collect();
        for (i, r, k) in std::mem::take(&mut self.pending_rel) {
            if let Some(&t) = keyed.get(&k) {
                match self.nodes[i].relations.iter_mut().find(|(rr, _)| *rr == r) {
                    Some((_, v)) => v.push(t),
                    None => self.nodes[i].relations.push((r, vec![t])),
                }
            }
        }
        let mut slots: Vec<Option<Node>> = self.nodes.into_iter().map(Some).collect();
        let nodes: Vec<Node> = order.into_iter().filter_map(|i| slots[i].take()).collect();
        let index = nodes.iter().enumerate().map(|(i, n)| (n.id, i)).collect();
        let focus = self.focus_key.as_ref().and_then(|k| keyed.get(k).copied());
        let mut t = Tree { revision: 0, nodes, index, focus, window: WindowInfo::default(), truncated: self.truncated };
        t.set_window(WindowInfo::default());
        t
    }
}

/// Gives `next` its revision: the previous one when nothing at all changed (`false` is returned and nothing needs
/// publishing), otherwise `prev.revision + 1`. Each node keeps its `changed` stamp unless its semantics changed.
pub fn stamp(prev: Option<&Tree>, next: &mut Tree) -> bool {
    let Some(prev) = prev else {
        next.revision = 1;
        for n in &mut next.nodes {
            n.changed = 1;
        }
        return true;
    };
    // `changed` is not part of what the app describes; compare as if it were equal.
    let rev = prev.revision + 1;
    let mut any = next.nodes.len() != prev.nodes.len() || next.focus != prev.focus || next.window != prev.window || next.truncated != prev.truncated;
    for n in &mut next.nodes {
        match prev.get(n.id) {
            Some(o) if o.same_semantics(n) => {
                n.changed = o.changed;
                if !any
                    && (o.states != n.states
                        || o.bounds != n.bounds
                        || o.text != n.text
                        || o.value != n.value
                        || o.attributes != n.attributes
                        || o.relations != n.relations
                        || o.parent != n.parent
                        || o.help != n.help
                        || o.actions != n.actions)
                {
                    any = true;
                }
            }
            _ => {
                n.changed = rev;
                any = true;
            }
        }
    }
    if !any {
        // Positional order could still differ (a node moved between parents keeps semantic equality of both).
        any = next.nodes.iter().zip(&prev.nodes).any(|(a, b)| a.id != b.id);
    }
    next.revision = if any { rev } else { prev.revision };
    any
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(ids: &mut Ids, checked: bool, extra: bool) -> Tree {
        let mut b = TreeBuilder::new(ids, "App");
        let f = b.add(NodeId::ROOT, Spec::new("frame", Role::Frame, "Win").bounds(Some((0, 0, 100, 100)))).unwrap();
        b.add(f, Spec::new("t", Role::Switch, "Wi-Fi").state(State::Checked, checked).state(State::Focusable, true).bounds(Some((10, 10, 40, 20)))).unwrap();
        if extra {
            b.add(f, Spec::new("x", Role::PushButton, "More")).unwrap();
        }
        b.focus("t");
        b.finish()
    }

    #[test]
    fn ids_are_stable_across_builds_and_never_reused() {
        let mut ids = Ids::default();
        let a = sample(&mut ids, false, true);
        let b = sample(&mut ids, true, false);
        assert_eq!(a.by_key("t").unwrap().id, b.by_key("t").unwrap().id);
        assert!(b.by_key("x").is_none());
        let c = sample(&mut ids, true, true);
        assert_eq!(a.by_key("x").unwrap().id, c.by_key("x").unwrap().id, "a key that comes back gets its old id");
        assert_eq!(ids.len(), 3);
        assert_eq!(a.root().unwrap().id, NodeId::ROOT);
        assert_eq!(a.index_in_parent(a.by_key("t").unwrap().id), 0);
    }

    #[test]
    fn duplicate_keys_get_deterministic_suffixes() {
        let mut ids = Ids::default();
        let mut b = TreeBuilder::new(&mut ids, "App");
        b.add(NodeId::ROOT, Spec::new("k", Role::Label, "one"));
        b.add(NodeId::ROOT, Spec::new("k", Role::Label, "two"));
        let t = b.finish();
        assert_eq!(t.by_key("k#2").unwrap().name, "two");
    }

    #[test]
    fn revisions_follow_semantic_changes_only() {
        let mut ids = Ids::default();
        let mut a = sample(&mut ids, false, false);
        assert!(stamp(None, &mut a));
        assert_eq!(a.revision, 1);
        let mut same = sample(&mut ids, false, false);
        assert!(!stamp(Some(&a), &mut same), "an identical rebuild publishes nothing");
        assert_eq!(same.revision, 1);
        // Focus/geometry change: new revision, but the switch's semantic stamp stays.
        let mut moved = sample(&mut ids, false, false);
        moved.set_window(WindowInfo { origin: Some((5, 5)), active: true });
        assert!(stamp(Some(&same), &mut moved));
        assert_eq!(moved.revision, 2);
        let t = moved.by_key("t").unwrap();
        assert_eq!(t.changed, 1);
        assert!(t.states.contains(State::Focused));
        // Value change: the switch is re-stamped.
        let mut flipped = sample(&mut ids, true, false);
        flipped.set_window(moved.window);
        assert!(stamp(Some(&moved), &mut flipped));
        assert_eq!(flipped.by_key("t").unwrap().changed, 3);
        assert_eq!(flipped.by_key("frame").unwrap().changed, 1);
    }

    #[test]
    fn budget_and_strings_are_bounded() {
        let mut ids = Ids::default();
        let mut b = TreeBuilder::new(&mut ids, "App");
        for i in 0..MAX_NODES + 50 {
            b.add(NodeId::ROOT, Spec::new(format!("n{i}"), Role::Label, "x".repeat(MAX_STRING + 10)));
        }
        let t = b.finish();
        assert_eq!(t.len(), MAX_NODES);
        assert!(t.truncated);
        assert!(t.nodes()[1].name.len() <= MAX_STRING);
        assert!(t.nodes()[1].name.ends_with('…'));
    }

    #[test]
    fn masked_text_never_carries_characters() {
        let t = TextInfo { content: TextContent::Masked { chars: 4 }, caret: 9, selection: Some((3, 1)), editable: true, multi_line: false };
        let mut ids = Ids::default();
        let mut b = TreeBuilder::new(&mut ids, "App");
        b.add(NodeId::ROOT, Spec::new("pw", Role::PasswordText, "Password").text(t));
        let tree = b.finish();
        let n = tree.by_key("pw").unwrap().text.clone().unwrap();
        assert_eq!(n.visible(), "••••");
        assert_eq!(n.caret, 4, "caret clamped to the visible length");
        assert_eq!(n.selection, None, "inverted selection dropped");
    }

    #[test]
    fn states_use_atspi_numbering() {
        let s = StateSet::of(&[State::Enabled, State::Focused, State::ReadOnly]);
        assert_eq!(s.words(), [(1 << 8) | (1 << 12), 1 << (43 - 32)]);
        assert_eq!(Role::Switch.atspi(), 130);
        assert_eq!(Role::PushButton.name(), "button");
    }
}
