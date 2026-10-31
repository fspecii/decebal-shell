//! Typed, bounded, immutable menu trees parsed from `com.canonical.dbusmenu` layouts.
//!
//! A [`MenuTree`] is never mutated after it is published: property updates build a new tree
//! ([`MenuTree::with_updates`]) and the worker swaps the `Arc`. Every string, list, depth and icon
//! is capped by [`Limits`]; malformed nodes are dropped (and counted) instead of failing the menu.

use super::limits::Limits;
use crate::dbus::Value;
use std::collections::HashSet;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ItemKind {
    #[default]
    Standard,
    Separator,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToggleType {
    #[default]
    None,
    /// `toggle-type = "checkmark"`.
    Check,
    /// `toggle-type = "radio"`.
    Radio,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToggleState {
    #[default]
    Off,
    On,
    /// Any `toggle-state` other than 0 or 1 (the spec's "indeterminate").
    Mixed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Disposition {
    #[default]
    Normal,
    Informative,
    Warning,
    Alert,
}

/// One exported item, with the protocol's defaults for every property the exporter left out.
#[derive(Debug, Clone, PartialEq)]
pub struct MenuItem {
    /// The exporter's item id: the only thing sent back in `Event`/`AboutToShow`.
    pub id: i32,
    pub kind: ItemKind,
    /// Raw label, still carrying `_` mnemonics (see [`MenuItem::display_label`]).
    pub label: String,
    pub enabled: bool,
    pub visible: bool,
    pub toggle_type: ToggleType,
    pub toggle_state: ToggleState,
    /// `children-display = "submenu"`: a submenu even while its children are not yet loaded.
    pub children_display_submenu: bool,
    /// `shortcut`: alternatives, each a list of modifier names plus a key (`[["Control", "S"]]`).
    pub shortcuts: Vec<Vec<String>>,
    pub icon_name: String,
    /// `icon-data`: PNG bytes, kept only when they carry a PNG signature and fit the caps.
    pub icon_png: Option<Arc<[u8]>>,
    pub accessible_desc: String,
    pub disposition: Disposition,
    pub children: Vec<MenuItem>,
}

impl MenuItem {
    pub fn new(id: i32) -> Self {
        Self {
            id,
            kind: ItemKind::Standard,
            label: String::new(),
            enabled: true,
            visible: true,
            toggle_type: ToggleType::None,
            toggle_state: ToggleState::Off,
            children_display_submenu: false,
            shortcuts: Vec::new(),
            icon_name: String::new(),
            icon_png: None,
            accessible_desc: String::new(),
            disposition: Disposition::Normal,
            children: Vec::new(),
        }
    }

    pub fn is_separator(&self) -> bool {
        self.kind == ItemKind::Separator
    }

    pub fn is_submenu(&self) -> bool {
        !self.is_separator() && (self.children_display_submenu || !self.children.is_empty())
    }

    /// The label without mnemonic markers: `_File` → `File`, `Save __As` → `Save _As`.
    pub fn display_label(&self) -> String {
        split_mnemonic(&self.label).0
    }

    /// The mnemonic character (`_File` → `f`), lower-cased, if the label marks one.
    pub fn mnemonic(&self) -> Option<char> {
        split_mnemonic(&self.label).1
    }

    /// The first shortcut in the bar's notation (`[["Control", "Shift", "s"]]` → `⌃⇧S`).
    pub fn shortcut_text(&self) -> String {
        self.shortcuts.first().map(|s| shortcut_text(s)).unwrap_or_default()
    }

    fn reset(&mut self, name: &str) {
        let d = MenuItem::new(self.id);
        match name {
            "type" => self.kind = d.kind,
            "label" => self.label = d.label,
            "enabled" => self.enabled = d.enabled,
            "visible" => self.visible = d.visible,
            "toggle-type" => self.toggle_type = d.toggle_type,
            "toggle-state" => self.toggle_state = d.toggle_state,
            "children-display" => self.children_display_submenu = d.children_display_submenu,
            "shortcut" => self.shortcuts = d.shortcuts,
            "icon-name" => self.icon_name = d.icon_name,
            "icon-data" => self.icon_png = None,
            "accessible-desc" => self.accessible_desc = d.accessible_desc,
            "disposition" => self.disposition = d.disposition,
            _ => {}
        }
    }

    /// Applies one property. Values of the wrong type are ignored (the old value stays).
    fn apply(&mut self, name: &str, value: &Value, limits: &Limits, icons: &mut usize) {
        let v = un(value);
        match name {
            "type" => {
                if let Some(s) = v_str(v) {
                    self.kind = if s == "separator" { ItemKind::Separator } else { ItemKind::Standard };
                }
            }
            "label" => {
                if let Some(s) = v_str(v) {
                    self.label = cap(s, limits.max_string);
                }
            }
            "enabled" => {
                if let Value::Bool(b) = v {
                    self.enabled = *b;
                }
            }
            "visible" => {
                if let Value::Bool(b) = v {
                    self.visible = *b;
                }
            }
            "toggle-type" => {
                if let Some(s) = v_str(v) {
                    self.toggle_type = match s {
                        "checkmark" => ToggleType::Check,
                        "radio" => ToggleType::Radio,
                        _ => ToggleType::None,
                    };
                }
            }
            "toggle-state" => {
                if let Some(n) = v_int(v) {
                    self.toggle_state = match n {
                        0 => ToggleState::Off,
                        1 => ToggleState::On,
                        _ => ToggleState::Mixed,
                    };
                }
            }
            "children-display" => {
                if let Some(s) = v_str(v) {
                    self.children_display_submenu = s == "submenu";
                }
            }
            "shortcut" => {
                if let Value::Array(alts) = v {
                    let mut out = Vec::new();
                    for alt in alts.iter().take(limits.max_shortcuts) {
                        let Value::Array(keys) = un(alt) else { return };
                        let keys: Vec<String> = keys.iter().take(limits.max_shortcut_keys).filter_map(|k| v_str(un(k)).map(|s| cap(s, 32))).filter(|s| !s.is_empty()).collect();
                        if !keys.is_empty() {
                            out.push(keys);
                        }
                    }
                    self.shortcuts = out;
                }
            }
            "icon-name" => {
                if let Some(s) = v_str(v) {
                    self.icon_name = cap(s, limits.max_string);
                }
            }
            "icon-data" => {
                if let Value::Bytes(b) = v {
                    let old = self.icon_png.as_ref().map_or(0, |p| p.len());
                    *icons -= old.min(*icons);
                    self.icon_png = None;
                    let png = b.len() >= 8 && b[..8] == [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
                    if png && b.len() <= limits.max_icon_bytes && *icons + b.len() <= limits.max_icon_total {
                        *icons += b.len();
                        self.icon_png = Some(Arc::from(b.as_slice()));
                    }
                }
            }
            "accessible-desc" => {
                if let Some(s) = v_str(v) {
                    self.accessible_desc = cap(s, limits.max_string);
                }
            }
            "disposition" => {
                if let Some(s) = v_str(v) {
                    self.disposition = match s {
                        "informative" => Disposition::Informative,
                        "warning" => Disposition::Warning,
                        "alert" => Disposition::Alert,
                        _ => Disposition::Normal,
                    };
                }
            }
            _ => {}
        }
    }

    fn find(&self, id: i32) -> Option<&MenuItem> {
        if self.id == id {
            return Some(self);
        }
        self.children.iter().find_map(|c| c.find(id))
    }

    fn find_mut(&mut self, id: i32) -> Option<&mut MenuItem> {
        if self.id == id {
            return Some(self);
        }
        self.children.iter_mut().find_map(|c| c.find_mut(id))
    }

    /// `id` below `self`, when it and every item above it (excluding `self`) is visible and enabled.
    fn reach(&self, id: i32) -> Option<&MenuItem> {
        for c in self.children.iter().filter(|c| c.visible && c.enabled) {
            if c.id == id {
                return Some(c);
            }
            if let Some(found) = c.reach(id) {
                return Some(found);
            }
        }
        None
    }

    fn count(&self) -> usize {
        1 + self.children.iter().map(MenuItem::count).sum::<usize>()
    }
}

/// Why a layout could not be used at all (individual bad nodes are dropped, not fatal).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// The reply is not `u(ia{sv}av)`.
    Shape(&'static str),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Shape(s) => write!(f, "malformed menu layout: {s}"),
        }
    }
}

/// An immutable, capped snapshot of one exporter's menu.
#[derive(Debug, Clone, PartialEq)]
pub struct MenuTree {
    /// The exporter's layout revision (`GetLayout`'s first value).
    pub revision: u32,
    pub root: MenuItem,
    /// Something was cut by a cap (items, depth, children).
    pub truncated: bool,
    /// Nodes refused as malformed or as duplicate ids.
    pub dropped: usize,
    icon_bytes: usize,
}

struct Ctx<'a> {
    limits: &'a Limits,
    seen: HashSet<i32>,
    items: usize,
    truncated: bool,
    dropped: usize,
    icons: usize,
}

impl MenuTree {
    /// Parses a `GetLayout` reply (`u` revision, `(ia{sv}av)` layout).
    pub fn from_layout(args: &[Value], limits: &Limits) -> Result<MenuTree, ParseError> {
        let revision = match args.first() {
            Some(Value::U32(r)) => *r,
            _ => return Err(ParseError::Shape("revision is not a uint32")),
        };
        let layout = args.get(1).ok_or(ParseError::Shape("missing layout"))?;
        let mut ctx = Ctx { limits, seen: HashSet::new(), items: 0, truncated: false, dropped: 0, icons: 0 };
        let root = node(layout, 0, &mut ctx).ok_or(ParseError::Shape("root item is malformed"))?;
        Ok(MenuTree { revision, root, truncated: ctx.truncated, dropped: ctx.dropped, icon_bytes: ctx.icons })
    }

    /// A tree from already-typed items (tests, previews). Ids must be unique; caps are not reapplied.
    pub fn from_items(revision: u32, root: MenuItem) -> MenuTree {
        MenuTree { revision, root, truncated: false, dropped: 0, icon_bytes: 0 }
    }

    pub fn find(&self, id: i32) -> Option<&MenuItem> {
        self.root.find(id)
    }

    pub fn len(&self) -> usize {
        self.root.count()
    }

    pub fn is_empty(&self) -> bool {
        self.root.children.is_empty()
    }

    /// Whether a click on `id` may be forwarded: it exists below the root, every item on its path
    /// is visible and enabled, and it is a plain item (not a separator or a submenu).
    pub fn activatable(&self, id: i32) -> bool {
        self.reachable(id).is_some_and(|it| !it.is_separator() && !it.is_submenu())
    }

    /// Whether `id` is a submenu the user can open (for `AboutToShow`/`opened`/`closed`).
    /// The root (id 0) always qualifies.
    pub fn openable(&self, id: i32) -> bool {
        id == self.root.id || self.reachable(id).is_some_and(MenuItem::is_submenu)
    }

    fn reachable(&self, id: i32) -> Option<&MenuItem> {
        self.root.reach(id)
    }

    /// Applies `ItemsPropertiesUpdated` (`a(ia{sv})` updated, `a(ias)` removed) and returns the new
    /// tree, or `None` when no known item changed. Unknown ids and wrong types are ignored.
    pub fn with_updates(&self, updated: &Value, removed: &Value, limits: &Limits) -> Option<MenuTree> {
        let mut next = self.clone();
        let mut icons = next.icon_bytes;
        let mut budget = limits.max_items;
        if let Value::Array(items) = un(removed) {
            for it in items {
                if budget == 0 {
                    break;
                }
                budget -= 1;
                let Value::Struct(f) = un(it) else { continue };
                let (Some(id), Some(Value::Array(names))) = (f.first().and_then(v_id), f.get(1).map(un)) else { continue };
                if let Some(item) = next.root.find_mut(id) {
                    for n in names.iter().take(32) {
                        if let Some(n) = v_str(un(n)) {
                            if n == "icon-data" {
                                icons -= item.icon_png.as_ref().map_or(0, |p| p.len()).min(icons);
                            }
                            item.reset(n);
                        }
                    }
                }
            }
        }
        if let Value::Array(items) = un(updated) {
            for it in items {
                if budget == 0 {
                    break;
                }
                budget -= 1;
                let Value::Struct(f) = un(it) else { continue };
                let (Some(id), Some(Value::Dict(props))) = (f.first().and_then(v_id), f.get(1).map(un)) else { continue };
                if let Some(item) = next.root.find_mut(id) {
                    for (k, v) in props.iter().take(32) {
                        if let Some(k) = v_str(k) {
                            item.apply(k, v, limits, &mut icons);
                        }
                    }
                    if item.is_separator() {
                        item.children.clear();
                    }
                }
            }
        }
        next.icon_bytes = icons;
        (next != *self).then_some(next)
    }
}

fn node(v: &Value, depth: usize, ctx: &mut Ctx<'_>) -> Option<MenuItem> {
    let Value::Struct(f) = un(v) else {
        ctx.dropped += 1;
        return None;
    };
    let (Some(id), Some(Value::Dict(props)), Some(Value::Array(children))) = (f.first().and_then(v_id), f.get(1).map(un), f.get(2).map(un)) else {
        ctx.dropped += 1;
        return None;
    };
    if f.len() != 3 || id < 0 {
        ctx.dropped += 1;
        return None;
    }
    if ctx.items >= ctx.limits.max_items {
        ctx.truncated = true;
        return None;
    }
    if !ctx.seen.insert(id) {
        ctx.dropped += 1;
        return None;
    }
    ctx.items += 1;
    let mut item = MenuItem::new(id);
    for (k, v) in props.iter().take(32) {
        if let Some(k) = v_str(k) {
            item.apply(k, v, ctx.limits, &mut ctx.icons);
        }
    }
    if item.is_separator() {
        return Some(item);
    }
    if depth >= ctx.limits.max_depth {
        ctx.truncated |= !children.is_empty();
        return Some(item);
    }
    if children.len() > ctx.limits.max_children {
        ctx.truncated = true;
    }
    for c in children.iter().take(ctx.limits.max_children) {
        if let Some(c) = node(c, depth + 1, ctx) {
            item.children.push(c);
        }
    }
    Some(item)
}

fn un(v: &Value) -> &Value {
    match v {
        Value::Variant(x) => un(x),
        _ => v,
    }
}

fn v_str(v: &Value) -> Option<&str> {
    match v {
        Value::Str(s) => Some(s),
        _ => None,
    }
}

fn v_int(v: &Value) -> Option<i64> {
    match v {
        Value::I32(_) | Value::U32(_) | Value::I16(_) | Value::U16(_) | Value::U8(_) | Value::I64(_) => v.as_int(),
        _ => None,
    }
}

fn v_id(v: &Value) -> Option<i32> {
    match un(v) {
        Value::I32(n) => Some(*n),
        _ => None,
    }
}

fn cap(s: &str, n: usize) -> String {
    s.chars().filter(|c| !c.is_control()).take(n).collect()
}

/// Splits dbusmenu's mnemonic markup: a single `_` marks the next character, `__` is a literal `_`.
pub fn split_mnemonic(label: &str) -> (String, Option<char>) {
    let mut out = String::with_capacity(label.len());
    let mut key = None;
    let mut chars = label.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '_' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('_') => out.push('_'),
            Some(n) => {
                if key.is_none() {
                    key = n.to_lowercase().next();
                }
                out.push(n);
            }
            None => out.push('_'),
        }
    }
    (out, key)
}

/// `["Control", "Shift", "s"]` → `⌃⇧S`; unknown modifiers are spelled out with a `+`.
pub fn shortcut_text(keys: &[String]) -> String {
    let mut out = String::new();
    let Some((key, mods)) = keys.split_last() else { return out };
    for m in mods {
        match m.as_str() {
            "Control" => out.push('⌃'),
            "Shift" => out.push('⇧'),
            "Alt" => out.push('⌥'),
            "Super" => out.push('⌘'),
            other => {
                out.push_str(other);
                out.push('+');
            }
        }
    }
    let mut cs = key.chars();
    match (cs.next(), cs.next()) {
        (Some(c), None) => out.extend(c.to_uppercase()),
        _ => out.push_str(key),
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> Value {
        Value::Variant(Box::new(Value::Str(v.into())))
    }
    fn b(v: bool) -> Value {
        Value::Variant(Box::new(Value::Bool(v)))
    }
    fn i(v: i32) -> Value {
        Value::Variant(Box::new(Value::I32(v)))
    }
    fn props(p: &[(&str, Value)]) -> Value {
        Value::Dict(p.iter().map(|(k, v)| (Value::Str((*k).into()), v.clone())).collect())
    }
    fn node(id: i32, p: &[(&str, Value)], children: Vec<Value>) -> Value {
        Value::Variant(Box::new(Value::Struct(vec![Value::I32(id), props(p), Value::Array(children)])))
    }
    fn layout(root: Value) -> Vec<Value> {
        let Value::Variant(inner) = root else { unreachable!() };
        vec![Value::U32(7), *inner]
    }

    fn sample() -> MenuTree {
        let file = node(
            1,
            &[("label", s("_File")), ("children-display", s("submenu"))],
            vec![
                node(10, &[("label", s("_Open…")), ("shortcut", Value::Variant(Box::new(Value::Array(vec![Value::Array(vec![Value::Str("Control".into()), Value::Str("o".into())])]))))], vec![]),
                node(11, &[("type", s("separator"))], vec![]),
                node(12, &[("label", s("Word _Wrap")), ("toggle-type", s("checkmark")), ("toggle-state", i(1))], vec![]),
                node(13, &[("label", s("Hidden")), ("visible", b(false))], vec![]),
                node(14, &[("label", s("Recent")), ("children-display", s("submenu"))], vec![node(140, &[("label", s("a.txt"))], vec![])]),
            ],
        );
        let edit = node(2, &[("label", s("_Edit")), ("enabled", b(false))], vec![node(20, &[("label", s("Undo"))], vec![])]);
        MenuTree::from_layout(&layout(node(0, &[], vec![file, edit])), &Limits::default()).unwrap()
    }

    #[test]
    fn parses_typed_properties_and_defaults() {
        let t = sample();
        assert_eq!(t.revision, 7);
        assert_eq!(t.len(), 10);
        let open = t.find(10).unwrap();
        assert_eq!(open.display_label(), "Open…");
        assert_eq!(open.mnemonic(), Some('o'));
        assert_eq!(open.shortcut_text(), "⌃O");
        assert!(open.enabled && open.visible && open.toggle_type == ToggleType::None);
        assert!(t.find(11).unwrap().is_separator());
        let wrap = t.find(12).unwrap();
        assert_eq!((wrap.toggle_type, wrap.toggle_state), (ToggleType::Check, ToggleState::On));
        assert!(!t.find(13).unwrap().visible);
        assert!(t.find(1).unwrap().is_submenu() && t.find(14).unwrap().is_submenu());
        assert!(!t.find(2).unwrap().enabled);
    }

    #[test]
    fn activation_requires_a_reachable_plain_item() {
        let t = sample();
        assert!(t.activatable(10));
        assert!(t.activatable(12));
        assert!(t.activatable(140), "nested submenu child");
        assert!(!t.activatable(11), "separator");
        assert!(!t.activatable(13), "invisible");
        assert!(!t.activatable(14), "submenu itself");
        assert!(!t.activatable(20), "child of a disabled menu");
        assert!(!t.activatable(0), "root");
        assert!(!t.activatable(999), "unknown id");
        assert!(t.openable(0) && t.openable(1) && t.openable(14));
        assert!(!t.openable(10) && !t.openable(2), "plain item / disabled menu");
    }

    #[test]
    fn caps_truncate_instead_of_growing() {
        let limits = Limits { max_items: 5, max_depth: 2, max_children: 3, max_string: 4, ..Limits::default() };
        let wide = node(0, &[], (1..=10).map(|n| node(n, &[("label", s("abcdefgh"))], vec![])).collect());
        let t = MenuTree::from_layout(&layout(wide), &limits).unwrap();
        assert!(t.truncated);
        assert_eq!(t.root.children.len(), 3);
        assert_eq!(t.root.children[0].label, "abcd");
        let mut deep = node(100, &[], vec![]);
        for n in (1..10).rev() {
            deep = node(n, &[], vec![deep]);
        }
        let t = MenuTree::from_layout(&layout(node(0, &[], vec![deep])), &limits).unwrap();
        assert!(t.truncated);
        assert_eq!(t.len(), 3, "root + depth 1 + depth 2");
        let many = node(0, &[], (1..=3).map(|n| node(n, &[], (0..3).map(|k| node(n * 10 + k, &[], vec![])).collect())).collect());
        let t = MenuTree::from_layout(&layout(many), &limits).unwrap();
        assert_eq!(t.len(), 5);
    }

    #[test]
    fn malformed_and_duplicate_nodes_are_dropped_not_fatal() {
        let bad_shape = Value::Variant(Box::new(Value::Str("not a node".into())));
        let wrong_id = Value::Variant(Box::new(Value::Struct(vec![Value::Str("1".into()), props(&[]), Value::Array(vec![])])));
        let negative = node(-4, &[], vec![]);
        let dup = node(1, &[("label", s("second"))], vec![]);
        let typed_wrong = node(5, &[("label", i(3)), ("enabled", s("yes")), ("toggle-state", s("1"))], vec![]);
        let t = MenuTree::from_layout(&layout(node(0, &[], vec![node(1, &[("label", s("first"))], vec![]), bad_shape, wrong_id, negative, dup, typed_wrong])), &Limits::default()).unwrap();
        assert_eq!(t.dropped, 4);
        assert_eq!(t.root.children.iter().map(|c| c.id).collect::<Vec<_>>(), [1, 5]);
        assert_eq!(t.find(1).unwrap().label, "first");
        let five = t.find(5).unwrap();
        assert!(five.label.is_empty() && five.enabled && five.toggle_state == ToggleState::Off, "wrong types keep defaults");
        assert!(MenuTree::from_layout(&[Value::Str("x".into())], &Limits::default()).is_err());
        assert!(MenuTree::from_layout(&[Value::U32(1), Value::Str("x".into())], &Limits::default()).is_err());
        assert!(MenuTree::from_layout(&[Value::U32(1)], &Limits::default()).is_err());
    }

    #[test]
    fn icons_need_png_signature_and_fit_budgets() {
        let png = |n: usize| {
            let mut v = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
            v.resize(n, 0);
            Value::Variant(Box::new(Value::Bytes(v)))
        };
        let limits = Limits { max_icon_bytes: 100, max_icon_total: 150, ..Limits::default() };
        let t = MenuTree::from_layout(
            &layout(node(
                0,
                &[],
                vec![
                    node(1, &[("icon-data", png(90))], vec![]),
                    node(2, &[("icon-data", png(90))], vec![]),
                    node(3, &[("icon-data", png(101))], vec![]),
                    node(4, &[("icon-data", Value::Variant(Box::new(Value::Bytes(vec![1, 2, 3]))))], vec![]),
                ],
            )),
            &limits,
        )
        .unwrap();
        assert_eq!(t.find(1).unwrap().icon_png.as_ref().map(|p| p.len()), Some(90));
        assert!(t.find(2).unwrap().icon_png.is_none(), "over the tree budget");
        assert!(t.find(3).unwrap().icon_png.is_none(), "over the item cap");
        assert!(t.find(4).unwrap().icon_png.is_none(), "not a PNG");
        // Removing the first icon frees the budget for an update to the second.
        let removed = Value::Array(vec![Value::Struct(vec![Value::I32(1), Value::Array(vec![Value::Str("icon-data".into())])])]);
        let updated = Value::Array(vec![Value::Struct(vec![Value::I32(2), props(&[("icon-data", png(90))])])]);
        let t2 = t.with_updates(&updated, &removed, &limits).unwrap();
        assert!(t2.find(1).unwrap().icon_png.is_none());
        assert_eq!(t2.find(2).unwrap().icon_png.as_ref().map(|p| p.len()), Some(90));
    }

    #[test]
    fn property_updates_build_a_new_tree_and_leave_the_old_one() {
        let t = sample();
        let updated = Value::Array(vec![
            Value::Struct(vec![Value::I32(12), props(&[("toggle-state", i(0)), ("label", s("Wrap"))])]),
            Value::Struct(vec![Value::I32(2), props(&[("enabled", b(true))])]),
            Value::Struct(vec![Value::I32(4242), props(&[("label", s("ghost"))])]),
        ]);
        let removed = Value::Array(vec![Value::Struct(vec![Value::I32(13), Value::Array(vec![Value::Str("visible".into())])])]);
        let t2 = t.with_updates(&updated, &removed, &Limits::default()).unwrap();
        assert_eq!(t2.find(12).unwrap().toggle_state, ToggleState::Off);
        assert_eq!(t2.find(12).unwrap().label, "Wrap");
        assert!(t2.find(13).unwrap().visible, "removed property returns to its default");
        assert!(t2.activatable(20));
        assert_eq!(t.find(12).unwrap().toggle_state, ToggleState::On, "the published tree is untouched");
        assert!(t.with_updates(&Value::Array(vec![]), &Value::Array(vec![]), &Limits::default()).is_none());
        assert!(t.with_updates(&Value::Str("junk".into()), &Value::U32(3), &Limits::default()).is_none());
    }

    #[test]
    fn mnemonics_and_shortcuts() {
        assert_eq!(split_mnemonic("_File"), ("File".into(), Some('f')));
        assert_eq!(split_mnemonic("Save __As_"), ("Save _As_".into(), None));
        assert_eq!(split_mnemonic("a_B_c"), ("aBc".into(), Some('b')));
        let k = |v: &[&str]| shortcut_text(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(k(&["Control", "Shift", "z"]), "⌃⇧Z");
        assert_eq!(k(&["Alt", "F4"]), "⌥F4");
        assert_eq!(k(&["Hyper", "x"]), "Hyper+X");
        assert_eq!(k(&[]), "");
    }

    #[test]
    fn control_characters_are_stripped_from_labels() {
        let t = MenuTree::from_layout(&layout(node(0, &[], vec![node(1, &[("label", s("Bad\u{1b}[2J\nLabel"))], vec![])])), &Limits::default()).unwrap();
        assert_eq!(t.find(1).unwrap().label, "Bad[2JLabel");
    }
}
