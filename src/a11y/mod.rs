//! Semantic accessibility (AT-SPI2) for Decebal windows.
//!
//! * [`tree`]: typed, immutable snapshots with stable node ids and revisions.
//! * [`action`]: typed requests, validation, stale/disabled rejection.
//! * [`atspi`]: the AT-SPI2 2.62 wire protocol (pure; answers from a snapshot, diffs snapshots into events).
//! * [`bridge`]: the worker thread that watches `org.a11y.Status`, owns the accessibility-bus connection and
//!   registers with the registry; bounded queues between it and the UI.
//! * [`host`]: what a window backend embeds on its UI thread.
//!
//! An app opts in by implementing [`crate::window_app::WindowApp::a11y_name`], `a11y_tree` and `a11y_action`.
//! See `docs/accessibility.md` for the protocol coverage, coordinates per backend and the known limits.

pub mod action;
pub mod atspi;
pub mod bridge;
pub mod host;
pub mod tree;

pub use action::{ActionError, Op, Request, Target};
pub use tree::{ActionDef, NodeId, Rect, Relation, Role, Spec, State, StateSet, TextContent, TextInfo, Tree, TreeBuilder, ValueInfo};

#[cfg(test)]
mod tests;
