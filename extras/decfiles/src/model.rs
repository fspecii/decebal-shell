use serde::Serialize;
use std::path::{Path, PathBuf};

pub const MAX_ENTRIES: usize = 8192;
pub const MAX_COLUMNS: usize = 16;
pub const MAX_HISTORY: usize = 64;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub directory: bool,
    pub size: u64,
    pub modified: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Column {
    pub path: PathBuf,
    pub entries: Vec<Entry>,
    pub selected: Option<PathBuf>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub enum Intent {
    Visit,
    Back,
    Forward,
    Refresh,
}

#[derive(Clone, Debug, Serialize)]
pub struct Request {
    pub revision: u64,
    pub path: PathBuf,
    pub intent: Intent,
}

#[derive(Default, Debug, Serialize)]
pub struct Model {
    pub columns: Vec<Column>,
    pub history: Vec<PathBuf>,
    pub cursor: usize,
    pub revision: u64,
    pub pending: Option<Request>,
    pub error: Option<String>,
}

impl Model {
    pub fn current(&self) -> Option<&Path> {
        self.columns.last().map(|column| column.path.as_path())
    }

    pub fn can_back(&self) -> bool {
        !self.history.is_empty() && self.cursor > 0
    }
    pub fn can_forward(&self) -> bool {
        self.cursor + 1 < self.history.len()
    }

    pub fn request(&mut self, path: PathBuf, intent: Intent) -> Request {
        self.revision = self.revision.wrapping_add(1);
        let request = Request { revision: self.revision, path, intent };
        self.pending = Some(request.clone());
        self.error = None;
        request
    }

    pub fn history_target(&self, intent: &Intent) -> Option<PathBuf> {
        match intent {
            Intent::Back if self.can_back() => self.history.get(self.cursor - 1).cloned(),
            Intent::Forward if self.can_forward() => self.history.get(self.cursor + 1).cloned(),
            _ => None,
        }
    }

    // Commit only a complete successful enumeration. Failure/cancellation never publishes
    // half a folder, and an older request cannot replace the current location.
    pub fn complete(&mut self, request: &Request, result: Result<Vec<Entry>, String>) -> bool {
        if self.pending.as_ref().map(|p| p.revision) != Some(request.revision) {
            return false;
        }
        self.pending = None;
        let mut entries = match result {
            Ok(entries) if entries.len() <= MAX_ENTRIES => entries,
            Ok(_) => {
                self.error = Some(format!("This folder exceeds the prototype limit of {MAX_ENTRIES} items. The previous folder is unchanged."));
                return true;
            }
            Err(error) => {
                self.error = Some(error);
                return true;
            }
        };
        entries.sort_by(|a, b| b.directory.cmp(&a.directory).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())).then_with(|| a.path.cmp(&b.path)));
        let old_selection = self.columns.iter().find(|c| c.path == request.path).and_then(|c| c.selected.clone());
        if let Some(index) = self.columns.iter().position(|column| column.path == request.path) {
            self.columns.truncate(index);
        } else if let Some(parent) = request.path.parent().and_then(|parent| self.columns.iter().position(|column| column.path == parent)) {
            self.columns.truncate(parent + 1);
        } else {
            self.columns.clear();
        }
        if let Some(parent) = self.columns.last_mut() {
            parent.selected = Some(request.path.clone());
        }
        let selected = old_selection.filter(|path| entries.iter().any(|entry| &entry.path == path));
        self.columns.push(Column { path: request.path.clone(), entries, selected });
        if self.columns.len() > MAX_COLUMNS {
            self.columns.remove(0);
        }
        match request.intent {
            Intent::Visit if self.history.get(self.cursor) != Some(&request.path) => {
                self.history.truncate(if self.history.is_empty() { 0 } else { self.cursor + 1 });
                self.history.push(request.path.clone());
                if self.history.len() > MAX_HISTORY {
                    self.history.remove(0);
                }
                self.cursor = self.history.len() - 1;
            }
            Intent::Back => self.cursor = self.cursor.saturating_sub(1),
            Intent::Forward => self.cursor = (self.cursor + 1).min(self.history.len().saturating_sub(1)),
            _ => {}
        }
        self.error = None;
        true
    }

    /// Opt-in fixture snapshot: display strings plus raw Unix bytes retain non-UTF8 identity.
    pub fn evidence(&self) -> serde_json::Value {
        use std::os::unix::ffi::OsStrExt;
        let path_text = |path: &Path| path.to_string_lossy().into_owned();
        let columns: Vec<_> = self
            .columns
            .iter()
            .map(|column| {
                let entries: Vec<_> = column
                    .entries
                    .iter()
                    .map(|entry| {
                        serde_json::json!({
                            "path":path_text(&entry.path), "path_bytes":entry.path.as_os_str().as_bytes(),
                            "name":entry.name, "directory":entry.directory, "size":entry.size
                        })
                    })
                    .collect();
                serde_json::json!({"path":path_text(&column.path), "path_bytes":column.path.as_os_str().as_bytes(),
                "selected":column.selected.as_deref().map(path_text),
                "selected_bytes":column.selected.as_deref().map(|path| path.as_os_str().as_bytes()), "entries":entries})
            })
            .collect();
        serde_json::json!({"columns":columns, "history":self.history.iter().map(|path| path_text(path)).collect::<Vec<_>>(),
            "history_bytes":self.history.iter().map(|path| path.as_os_str().as_bytes()).collect::<Vec<_>>(),
            "cursor":self.cursor,"revision":self.revision,"pending":self.pending.as_ref().map(|request| serde_json::json!({"revision":request.revision,"path":path_text(&request.path)})),"error":self.error})
    }

    pub fn select(&mut self, column: usize, path: Option<PathBuf>) {
        if let Some(column) = self.columns.get_mut(column) {
            column.selected = path.filter(|path| column.entries.iter().any(|entry| &entry.path == path));
        }
    }
}

pub fn location(text: &str, current: &Path, home: &Path) -> Result<PathBuf, String> {
    if text.is_empty() {
        return Err("Enter a folder location.".into());
    }
    if text.contains('\0') {
        return Err("A folder location cannot contain a NUL character.".into());
    }
    if text.contains("://") {
        return Err("This prototype supports local folders only.".into());
    }
    let raw = if text == "~" {
        home.to_path_buf()
    } else if let Some(tail) = text.strip_prefix("~/") {
        home.join(tail)
    } else {
        PathBuf::from(text)
    };
    let joined = if raw.is_absolute() { raw } else { current.join(raw) };
    // Keep link/.. intact for off-thread physical resolution before GIO.
    Ok(joined)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entries(parent: &str) -> Vec<Entry> {
        vec![Entry { path: Path::new(parent).join("child"), name: "child".into(), directory: true, size: 0, modified: 0 }]
    }
    fn visit(model: &mut Model, path: &str) {
        let request = model.request(path.into(), Intent::Visit);
        assert!(model.complete(&request, Ok(entries(path))));
    }
    #[test]
    fn history_and_real_column_chain() {
        let mut model = Model::default();
        visit(&mut model, "/base");
        visit(&mut model, "/base/child");
        assert_eq!(model.columns.len(), 2);
        assert_eq!(model.columns[0].selected, Some("/base/child".into()));
        let request = model.request(model.history_target(&Intent::Back).unwrap(), Intent::Back);
        model.complete(&request, Ok(entries("/base")));
        assert_eq!(model.current(), Some(Path::new("/base")));
        assert!(model.can_forward());
        let request = model.request(model.history_target(&Intent::Forward).unwrap(), Intent::Forward);
        model.complete(&request, Ok(entries("/base/child")));
        assert_eq!(model.columns.len(), 2);
    }
    #[test]
    fn errors_and_stale_callbacks_leave_navigation_unchanged() {
        let mut model = Model::default();
        visit(&mut model, "/base");
        let old = model.request("/old".into(), Intent::Visit);
        let current = model.request("/missing".into(), Intent::Visit);
        assert!(!model.complete(&old, Ok(entries("/old"))));
        assert!(model.complete(&current, Err("Folder is unavailable".into())));
        assert_eq!(model.current(), Some(Path::new("/base")));
        assert_eq!(model.history.len(), 1);
    }
    #[test]
    fn location_is_local_and_preserves_literal_names() {
        assert_eq!(location("../folder # %", Path::new("/base/child"), Path::new("/home/test")).unwrap(), Path::new("/base/child/../folder # %"));
        assert_eq!(location("~/Notes", Path::new("/"), Path::new("/home/test")).unwrap(), Path::new("/home/test/Notes"));
        assert!(location("smb://server/share", Path::new("/"), Path::new("/home/test")).is_err());
    }
    #[test]
    fn new_branch_discards_forward_history_and_selection_is_validated() {
        let mut model = Model::default();
        visit(&mut model, "/a");
        visit(&mut model, "/b");
        let back = model.request("/a".into(), Intent::Back);
        model.complete(&back, Ok(entries("/a")));
        visit(&mut model, "/c");
        assert!(!model.can_forward());
        assert_eq!(model.history, [PathBuf::from("/a"), PathBuf::from("/c")]);
        model.select(0, Some("/not-an-entry".into()));
        assert!(model.columns[0].selected.is_none());
    }

    #[test]
    fn selecting_a_sibling_in_an_ancestor_column_preserves_that_column() {
        let mut model = Model::default();
        let first = model.request("/base".into(), Intent::Visit);
        let mut root_entries = entries("/base");
        root_entries.push(Entry { path: "/base/other".into(), name: "other".into(), directory: true, size: 0, modified: 0 });
        model.complete(&first, Ok(root_entries));
        visit(&mut model, "/base/child");
        visit(&mut model, "/base/child/child");
        visit(&mut model, "/base/other");
        assert_eq!(model.columns.len(), 2);
        assert_eq!(model.columns[0].path, Path::new("/base"));
        assert_eq!(model.columns[0].selected, Some("/base/other".into()));
    }
    #[test]
    fn repeated_pending_history_requests_only_commit_the_latest_cursor_change() {
        let mut model = Model::default();
        visit(&mut model, "/a");
        visit(&mut model, "/b");
        visit(&mut model, "/c");
        let first = model.request(model.history_target(&Intent::Back).unwrap(), Intent::Back);
        let second = model.request(model.history_target(&Intent::Back).unwrap(), Intent::Back);
        assert!(!model.complete(&first, Ok(entries("/b"))));
        assert!(model.complete(&second, Ok(entries("/b"))));
        assert_eq!(model.cursor, 1);
        assert_eq!(model.current(), Some(Path::new("/b")));
        let first = model.request(model.history_target(&Intent::Forward).unwrap(), Intent::Forward);
        let second = model.request(model.history_target(&Intent::Forward).unwrap(), Intent::Forward);
        assert!(!model.complete(&first, Ok(entries("/c"))));
        assert!(model.complete(&second, Ok(entries("/c"))));
        assert_eq!(model.cursor, 2);
        assert_eq!(model.history.len(), 3);
    }
    #[test]
    fn fixture_snapshot_retains_non_utf8_path_bytes() {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        let path = PathBuf::from(std::ffi::OsString::from_vec(b"/base/\xff".to_vec()));
        let mut model = Model::default();
        let request = model.request("/base".into(), Intent::Visit);
        model.complete(&request, Ok(vec![Entry { path: path.clone(), name: "display".into(), directory: false, size: 0, modified: 0 }]));
        model.select(0, Some(path.clone()));
        let snapshot = model.evidence();
        assert_eq!(snapshot["columns"][0]["selected_bytes"], serde_json::json!(path.as_os_str().as_bytes()));
        assert!(serde_json::to_string(&snapshot).is_ok());
    }

    #[test]
    fn enumeration_and_history_are_bounded() {
        let mut model = Model::default();
        visit(&mut model, "/base");
        let request = model.request("/huge".into(), Intent::Visit);
        model.complete(&request, Ok(vec![entries("/huge")[0].clone(); MAX_ENTRIES + 1]));
        assert_eq!(model.current(), Some(Path::new("/base")));
        assert!(model.error.is_some());
        for i in 0..100 {
            visit(&mut model, &format!("/folder{i}"));
        }
        assert_eq!(model.history.len(), MAX_HISTORY);
    }
}
