//! Hard caps on everything an application (or any other bus peer) can make the menu worker hold.

use std::time::Duration;

/// Bounds applied while parsing exported menus and serving the registrar. Values past a cap are
/// truncated or refused; they never grow memory, queues or recursion without limit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limits {
    /// Deepest submenu level kept below the root (the root's children are depth 1).
    pub max_depth: usize,
    /// Items kept per menu tree, including separators and the root.
    pub max_items: usize,
    /// Children kept per item.
    pub max_children: usize,
    /// Characters kept from a label, icon name or shortcut key.
    pub max_string: usize,
    /// PNG bytes kept from one item's `icon-data`.
    pub max_icon_bytes: usize,
    /// PNG bytes kept across one whole tree.
    pub max_icon_total: usize,
    /// Shortcuts per item and keys per shortcut.
    pub max_shortcuts: usize,
    pub max_shortcut_keys: usize,
    /// Largest reply body accepted from an exporter (`GetLayout`) or registrar.
    pub max_reply_bytes: usize,
    /// Bytes buffered from the bus before the connection is treated as hostile and dropped.
    pub max_buffered: usize,
    /// Method calls this worker may have in flight at once.
    pub max_pending: usize,
    /// Commands (open/close/activate) queued from the UI before `try_send` refuses more.
    pub command_queue: usize,
    /// Windows the owned registrar keeps in total and per sender.
    pub max_registrations: usize,
    pub max_registrations_per_sender: usize,
    /// How long a method call may stay unanswered.
    pub call_timeout: Duration,
    /// Coalescing window for `LayoutUpdated` storms.
    pub layout_debounce: Duration,
    /// One absolute budget for connecting (every address candidate), authenticating and `Hello`.
    /// A stop cuts it short.
    pub connect_timeout: Duration,
    /// One absolute budget per outgoing message while the bus is not draining our socket.
    pub write_timeout: Duration,
    /// Longest wait between reconnection attempts.
    pub max_backoff: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_depth: 8,
            max_items: 1024,
            max_children: 256,
            max_string: 256,
            max_icon_bytes: 64 << 10,
            max_icon_total: 1 << 20,
            max_shortcuts: 4,
            max_shortcut_keys: 6,
            max_reply_bytes: 4 << 20,
            max_buffered: 20 << 20,
            max_pending: 32,
            command_queue: 32,
            max_registrations: 512,
            max_registrations_per_sender: 64,
            call_timeout: Duration::from_secs(3),
            layout_debounce: Duration::from_millis(25),
            connect_timeout: Duration::from_secs(2),
            write_timeout: Duration::from_secs(2),
            max_backoff: Duration::from_secs(30),
        }
    }
}
