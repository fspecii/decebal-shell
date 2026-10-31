//! Typed accessibility actions and their validation against the current snapshot.
//!
//! The bridge turns AT-SPI calls into [`Request`]s stamped with the revision the assistive technology was served.
//! On the UI thread [`validate`] checks them against the window's *current* tree before the app's semantic
//! dispatcher sees them: a node that no longer exists, changed since the request was made, is disabled, or does not
//! offer the operation is rejected with a clear [`ActionError`]. Nothing here synthesises pointer or key events.

use super::tree::{Node, NodeId, Role, State, Tree};
use std::fmt;

/// What an assistive technology asked a node to do.
#[derive(Clone, PartialEq)]
pub enum Op {
    /// Action interface: run the named action (`click`, `toggle`, `press`, `activate`, `increment`, ...).
    Action(&'static str),
    /// Value interface: set the current value.
    SetValue(f64),
    /// Component.GrabFocus: move keyboard focus here (and scroll it into view).
    Focus,
    /// Component.ScrollTo / Text.ScrollSubstringTo: bring the node into view.
    ScrollTo,
    /// Selection.SelectChild on a single-choice container: choose child `index`.
    SelectChild(usize),
    /// Text.SetCaretOffset (characters).
    SetCaret(usize),
    /// Text.SetSelection / AddSelection (characters, start < end); `None` clears (RemoveSelection).
    SetSelection(Option<(usize, usize)>),
    /// EditableText.SetTextContents: replace the draft text.
    SetText(String),
    /// EditableText.InsertText at a character offset.
    InsertText { offset: usize, text: String },
    /// EditableText.DeleteText / CutText (character range).
    DeleteText { start: usize, end: usize, cut: bool },
    /// EditableText.CopyText.
    CopyText { start: usize, end: usize },
    /// EditableText.PasteText at a character offset.
    PasteText { offset: usize },
}

impl fmt::Debug for Op {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Action(_) => "Action",
            Self::SetValue(_) => "SetValue",
            Self::Focus => "Focus",
            Self::ScrollTo => "ScrollTo",
            Self::SelectChild(_) => "SelectChild",
            Self::SetCaret(_) => "SetCaret",
            Self::SetSelection(_) => "SetSelection",
            Self::SetText(_) => "SetText(<redacted>)",
            Self::InsertText { .. } => "InsertText(<redacted>)",
            Self::DeleteText { .. } => "DeleteText",
            Self::CopyText { .. } => "CopyText",
            Self::PasteText { .. } => "PasteText",
        })
    }
}

impl Op {
    /// Whether the operation changes the app (as opposed to focus or scrolling, which disabled nodes still allow).
    pub fn mutates(&self) -> bool {
        !matches!(self, Op::Focus | Op::ScrollTo | Op::CopyText { .. })
    }
}

/// A request from the bridge. `revision` is the snapshot revision the AT was served when it made the call.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub id: u64,
    pub revision: u64,
    pub node: NodeId,
    pub op: Op,
    /// After this instant the bridge has already answered "timed out": the UI must drop the request, not apply it.
    pub expires: Option<std::time::Instant>,
}

/// What the app's dispatcher receives once a request passed [`validate`]: the node as it is now.
#[derive(Debug, Clone, Copy)]
pub struct Target<'a> {
    pub node: &'a Node,
    pub op: &'a Op,
}

impl Target<'_> {
    pub fn key(&self) -> &str {
        &self.node.key
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ActionError {
    /// The node is gone from the current tree.
    UnknownNode,
    /// The node changed after the AT read it (its revision is newer than the request's).
    Stale { node_revision: u64, request_revision: u64 },
    /// The control is disabled.
    Disabled,
    /// The node does not offer this operation.
    Unsupported(String),
    /// The arguments are invalid (out of range, NaN, bad offsets, masked text...).
    Invalid(String),
    /// The app's model refused the change (its own validation, a conflict, an error notice).
    Refused(String),
    /// Too many requests are pending.
    Busy,
    /// The window or bridge is shutting down, or the reply was lost.
    Gone,
}

impl ActionError {
    /// The D-Bus error name the bridge replies with.
    pub fn dbus_name(&self) -> &'static str {
        match self {
            ActionError::UnknownNode => "org.decebal.Accessibility.Error.UnknownNode",
            ActionError::Stale { .. } => "org.decebal.Accessibility.Error.Stale",
            ActionError::Disabled => "org.decebal.Accessibility.Error.Disabled",
            ActionError::Unsupported(_) => "org.decebal.Accessibility.Error.Unsupported",
            ActionError::Invalid(_) => "org.freedesktop.DBus.Error.InvalidArgs",
            ActionError::Refused(_) => "org.decebal.Accessibility.Error.Refused",
            ActionError::Busy => "org.freedesktop.DBus.Error.LimitsExceeded",
            ActionError::Gone => "org.decebal.Accessibility.Error.Gone",
        }
    }
}

impl fmt::Display for ActionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ActionError::UnknownNode => write!(f, "this object no longer exists; read the tree again"),
            ActionError::Stale { node_revision, request_revision } => {
                write!(f, "this object changed (revision {node_revision}) after it was read (revision {request_revision}); read it again before acting")
            }
            ActionError::Disabled => write!(f, "this control is disabled"),
            ActionError::Unsupported(w) => write!(f, "not supported: {w}"),
            ActionError::Invalid(w) => write!(f, "invalid request: {w}"),
            ActionError::Refused(w) => write!(f, "refused: {w}"),
            ActionError::Busy => write!(f, "too many accessibility requests are pending; retry"),
            ActionError::Gone => write!(f, "the window is closing"),
        }
    }
}

/// Checks `req` against the current tree; on success the app may dispatch it.
pub fn validate<'a>(tree: &'a Tree, req: &'a Request) -> Result<Target<'a>, ActionError> {
    let node = tree.get(req.node).ok_or(ActionError::UnknownNode)?;
    if node.changed > req.revision {
        return Err(ActionError::Stale { node_revision: node.changed, request_revision: req.revision });
    }
    if req.op.mutates() && !node.enabled() {
        return Err(ActionError::Disabled);
    }
    match &req.op {
        Op::Action(name) => {
            if !node.actions.iter().any(|a| a.name == *name) {
                return Err(ActionError::Unsupported(format!("{} has no action {name:?}", node.role.name())));
            }
        }
        Op::SetValue(v) => {
            let Some(val) = &node.value else { return Err(ActionError::Unsupported(format!("{} has no value", node.role.name()))) };
            if !v.is_finite() {
                return Err(ActionError::Invalid("value is not a finite number".into()));
            }
            if *v < val.min - 1e-9 || *v > val.max + 1e-9 {
                return Err(ActionError::Invalid(format!("{v} is outside {}..{}", val.min, val.max)));
            }
            if node.states.contains(State::ReadOnly) {
                return Err(ActionError::Unsupported("the value is read-only".into()));
            }
        }
        Op::Focus => {
            if !node.states.contains(State::Focusable) {
                return Err(ActionError::Unsupported(format!("{} cannot take focus", node.role.name())));
            }
        }
        Op::ScrollTo => {}
        Op::SelectChild(i) => {
            if !node.selection {
                return Err(ActionError::Unsupported(format!("{} has no selectable children", node.role.name())));
            }
            let child = node.children.get(*i).and_then(|c| tree.get(*c)).ok_or_else(|| ActionError::Invalid(format!("no child {i}")))?;
            if !child.states.contains(State::Selectable) {
                return Err(ActionError::Invalid(format!("child {i} is not selectable")));
            }
            if !child.enabled() {
                return Err(ActionError::Disabled);
            }
        }
        Op::SetCaret(o) | Op::PasteText { offset: o } => {
            let t = node.text.as_ref().ok_or_else(|| ActionError::Unsupported("no text".into()))?;
            if *o > t.char_count() {
                return Err(ActionError::Invalid(format!("offset {o} is past the end ({})", t.char_count())));
            }
            if matches!(req.op, Op::PasteText { .. }) && !t.editable {
                return Err(ActionError::Unsupported("the text is not editable".into()));
            }
        }
        Op::SetSelection(r) => {
            let t = node.text.as_ref().ok_or_else(|| ActionError::Unsupported("no text".into()))?;
            if let Some((a, b)) = r {
                if a >= b || *b > t.char_count() {
                    return Err(ActionError::Invalid(format!("selection {a}..{b} is empty or past the end ({})", t.char_count())));
                }
            }
        }
        Op::SetText(_) | Op::InsertText { .. } | Op::DeleteText { .. } => {
            let t = node.text.as_ref().ok_or_else(|| ActionError::Unsupported("no text".into()))?;
            if !t.editable || node.states.contains(State::ReadOnly) {
                return Err(ActionError::Unsupported("the text is not editable".into()));
            }
            match &req.op {
                Op::InsertText { offset, .. } if *offset > t.char_count() => return Err(ActionError::Invalid(format!("offset {offset} is past the end ({})", t.char_count()))),
                Op::DeleteText { start, end, cut } => {
                    if start > end || *end > t.char_count() {
                        return Err(ActionError::Invalid(format!("range {start}..{end} is outside the text ({})", t.char_count())));
                    }
                    if *cut && t.masked() {
                        return Err(ActionError::Unsupported("a password field cannot be cut to the clipboard".into()));
                    }
                }
                _ => {}
            }
        }
        Op::CopyText { start, end } => {
            let t = node.text.as_ref().ok_or_else(|| ActionError::Unsupported("no text".into()))?;
            if t.masked() {
                return Err(ActionError::Unsupported("a password field cannot be copied".into()));
            }
            if start > end || *end > t.char_count() {
                return Err(ActionError::Invalid(format!("range {start}..{end} is outside the text ({})", t.char_count())));
            }
        }
    }
    if node.role == Role::Application {
        return Err(ActionError::Unsupported("the application object has no actions".into()));
    }
    Ok(Target { node, op: &req.op })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::a11y::tree::{stamp, ActionDef, Ids, Spec, TextContent, TextInfo, TreeBuilder, ValueInfo};

    fn tree(ids: &mut Ids, on: bool, slider_enabled: bool) -> Tree {
        let mut b = TreeBuilder::new(ids, "App");
        let f = b.add(NodeId::ROOT, Spec::new("f", Role::Frame, "W")).unwrap();
        b.add(f, Spec::new("sw", Role::Switch, "Wi-Fi").state(State::Checked, on).state(State::Focusable, true).action(ActionDef::new("toggle", "Toggle"))).unwrap();
        b.add(f, Spec::new("sl", Role::Slider, "Gap").enabled(slider_enabled).value(ValueInfo { current: 4.0, min: 0.0, max: 10.0, step: 1.0, text: "4".into() })).unwrap();
        let l = b.add(f, Spec::new("list", Role::List, "Pages").selection()).unwrap();
        b.add(l, Spec::new("p0", Role::ListItem, "A").state(State::Selectable, true)).unwrap();
        b.add(l, Spec::new("p1", Role::ListItem, "B")).unwrap();
        b.add(f, Spec::new("pw", Role::PasswordText, "Password").text(TextInfo { content: TextContent::Masked { chars: 3 }, caret: 3, selection: None, editable: true, multi_line: false })).unwrap();
        b.finish()
    }

    fn req(t: &Tree, key: &str, rev: u64, op: Op) -> Request {
        Request { id: 1, revision: rev, node: t.by_key(key).map(|n| n.id).unwrap_or(NodeId(999)), op, expires: None }
    }

    #[test]
    fn accepts_current_requests() {
        let mut ids = Ids::default();
        let mut t = tree(&mut ids, false, true);
        stamp(None, &mut t);
        let r = req(&t, "sw", 1, Op::Action("toggle"));
        assert_eq!(validate(&t, &r).unwrap().key(), "sw");
        assert!(validate(&t, &req(&t, "sl", 1, Op::SetValue(7.0))).is_ok());
        assert!(validate(&t, &req(&t, "list", 1, Op::SelectChild(0))).is_ok());
        assert!(validate(&t, &req(&t, "sw", 1, Op::Focus)).is_ok());
    }

    #[test]
    fn rejects_stale_unknown_and_disabled() {
        let mut ids = Ids::default();
        let mut a = tree(&mut ids, false, true);
        stamp(None, &mut a);
        let mut b = tree(&mut ids, true, false);
        stamp(Some(&a), &mut b);
        // The AT read revision 1; the switch changed at revision 2.
        assert!(matches!(validate(&b, &req(&b, "sw", 1, Op::Action("toggle"))), Err(ActionError::Stale { node_revision: 2, request_revision: 1 })));
        assert!(validate(&b, &req(&b, "sw", 2, Op::Action("toggle"))).is_ok());
        assert_eq!(validate(&b, &req(&b, "sl", 2, Op::SetValue(3.0))).unwrap_err(), ActionError::Disabled);
        assert_eq!(validate(&b, &req(&b, "gone", 2, Op::Focus)).unwrap_err(), ActionError::UnknownNode);
        // An unchanged node is not stale just because something else changed.
        assert!(validate(&b, &req(&b, "list", 1, Op::SelectChild(0))).is_ok());
    }

    #[test]
    fn rejects_unsupported_and_invalid_arguments() {
        let mut ids = Ids::default();
        let mut t = tree(&mut ids, false, true);
        stamp(None, &mut t);
        assert!(matches!(validate(&t, &req(&t, "sw", 1, Op::Action("click"))), Err(ActionError::Unsupported(_))));
        assert!(matches!(validate(&t, &req(&t, "sl", 1, Op::SetValue(f64::NAN))), Err(ActionError::Invalid(_))));
        assert!(matches!(validate(&t, &req(&t, "sl", 1, Op::SetValue(11.0))), Err(ActionError::Invalid(_))));
        assert!(matches!(validate(&t, &req(&t, "list", 1, Op::SelectChild(1))), Err(ActionError::Invalid(_))), "not selectable");
        assert!(matches!(validate(&t, &req(&t, "list", 1, Op::SelectChild(5))), Err(ActionError::Invalid(_))));
        assert!(matches!(validate(&t, &req(&t, "pw", 1, Op::CopyText { start: 0, end: 1 })), Err(ActionError::Unsupported(_))), "passwords are never copied");
        assert!(matches!(validate(&t, &req(&t, "pw", 1, Op::DeleteText { start: 0, end: 1, cut: true })), Err(ActionError::Unsupported(_))));
        assert!(validate(&t, &req(&t, "pw", 1, Op::DeleteText { start: 0, end: 1, cut: false })).is_ok());
        assert!(matches!(validate(&t, &req(&t, "pw", 1, Op::InsertText { offset: 9, text: "x".into() })), Err(ActionError::Invalid(_))));
        assert!(matches!(validate(&t, &req(&t, "sl", 1, Op::Focus)), Err(ActionError::Unsupported(_))));
        assert_eq!(ActionError::Busy.dbus_name(), "org.freedesktop.DBus.Error.LimitsExceeded");
    }
}
