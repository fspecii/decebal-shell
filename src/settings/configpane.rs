//! The "Configuration" pane: edit the raw configuration files as text, and keep named presets of the whole setup that
//! can be exported, imported and shared (`docs/presets.md`).
//!
//! * Edit files: a text editor ([`super::editor`]) per file (decebal.json, dock-settings.json, pinned apps, the rdwm
//!   config) plus a read-only overview of every effective setting. Saving validates first and shows problems with line
//!   numbers; an invalid file is never written; every save keeps a timestamped backup and "Revert" restores it.
//! * Presets: save the current setup, apply (always reversible), rename, duplicate, delete, export/import bundles.
//!
//! All the real work is in [`crate::preset`]; this file is state, the item lists and the event glue.

use super::editor::{Editor, Geo, Lang, Pos};
use super::io::{self, Stamp};
use super::model::{CtlVal, Model};
use super::ui::{Ctl, Frame, Item, Kind, Painter, Row};
use crate::preset::bundle;
use crate::preset::fsx;
use crate::preset::manifest::{self, Origin};
use crate::preset::parts::{self, Part, PartSet};
use crate::preset::store::{self, CaptureOpts, Env, Plan, Preset, Status};
use crate::preset::validate::{self, Diag, FileKind};
use crate::window_app::{Key, KeyEvent, Response};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Presets and the folders that did not load (folder, reason).
type Listing = (Vec<Preset>, Vec<(String, String)>);

pub const EDITOR_ID: &str = "cfg:editor";
const EDITOR_H: f32 = 400.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tab {
    Shell,
    Dock,
    Pinned,
    Rdwm,
    All,
}

impl Tab {
    pub const ALL: [Tab; 5] = [Tab::Shell, Tab::Dock, Tab::Pinned, Tab::Rdwm, Tab::All];
    pub fn id(self) -> &'static str {
        match self {
            Tab::Shell => "shell",
            Tab::Dock => "dock",
            Tab::Pinned => "pinned",
            Tab::Rdwm => "rdwm",
            Tab::All => "all",
        }
    }
    fn title(self) -> &'static str {
        match self {
            Tab::Shell => "Shell",
            Tab::Dock => "Dock",
            Tab::Pinned => "Pinned apps",
            Tab::Rdwm => "Window manager",
            Tab::All => "All settings",
        }
    }
    fn parse(s: &str) -> Option<Tab> {
        Tab::ALL.into_iter().find(|t| t.id() == s)
    }
    fn kind(self) -> Option<FileKind> {
        match self {
            Tab::Shell => Some(FileKind::Shell),
            Tab::Dock => Some(FileKind::Dock),
            Tab::Pinned => Some(FileKind::Pinned),
            Tab::Rdwm => Some(FileKind::Rdwm),
            Tab::All => None,
        }
    }
    fn lang(self) -> Lang {
        match self {
            Tab::Rdwm | Tab::All => Lang::Toml,
            _ => Lang::Json,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Edit,
    Presets,
    Save,
    Import,
    Apply,
}

struct Buf {
    ed: Editor,
    stamp: Stamp,
    errors: Vec<Diag>,
    /// The file changed on disk while there are unsaved edits.
    disk_changed: bool,
    /// What the last save did ("Saved. Backup: ...").
    note: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Purpose {
    Import,
    Export,
}

enum Ev {
    Picked(Purpose, Option<PathBuf>),
    Clip(Result<String, String>),
}

pub struct SaveForm {
    pub name: String,
    pub description: String,
    pub parts: PartSet,
    pub embed: bool,
}

struct ImportState {
    preset: Preset,
    plan: Plan,
    keep_commands: bool,
    source: String,
}

struct ApplyState {
    preset: Preset,
    plan: Plan,
}

pub struct State {
    pub screen: Screen,
    pub tab: Tab,
    bufs: HashMap<Tab, Buf>,
    /// Keyboard focus is in the editor.
    pub active: bool,
    pub drag: bool,
    last_click: Option<(std::time::Instant, f32, f32, u8)>,
    env: Env,
    listing: Option<Listing>,
    pub selected: Option<String>,
    pub form: SaveForm,
    rename_text: String,
    export_path: String,
    import_path: String,
    import: Option<ImportState>,
    apply: Option<ApplyState>,
    /// (preset name, restore point id) of the last apply, for "Undo".
    last_applied: Option<(String, String)>,
    events: Arc<Mutex<VecDeque<Ev>>>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl State {
    pub fn new(env: Env, wake: Arc<dyn Fn() + Send + Sync>) -> State {
        let home = std::env::var("HOME").unwrap_or_default();
        State {
            screen: Screen::Edit,
            tab: Tab::Shell,
            bufs: HashMap::new(),
            active: false,
            drag: false,
            last_click: None,
            env,
            listing: None,
            selected: None,
            form: SaveForm { name: String::new(), description: String::new(), parts: PartSet::defaults(), embed: false },
            rename_text: String::new(),
            export_path: format!("{home}/"),
            import_path: String::new(),
            import: None,
            apply: None,
            last_applied: None,
            events: Arc::new(Mutex::new(VecDeque::new())),
            wake,
        }
    }

    pub fn env(&self) -> &Env {
        &self.env
    }

    fn presets(&mut self) -> &Listing {
        if self.listing.is_none() {
            self.listing = Some(store::list(&self.env));
        }
        self.listing.as_ref().expect("just filled")
    }

    fn forget_listing(&mut self) {
        self.listing = None;
    }

    fn selected_preset(&mut self) -> Option<Preset> {
        let id = self.selected.clone()?;
        self.presets().0.iter().find(|p| p.id == id).cloned()
    }

    fn path_of(&self, tab: Tab) -> Option<PathBuf> {
        Some(match tab {
            Tab::Shell => self.env.shell_dir.join("decebal.json"),
            Tab::Dock => self.env.shell_dir.join("dock-settings.json"),
            Tab::Pinned => self.env.shell_dir.join("dock-pinned-macos.json"),
            Tab::Rdwm => self.env.rdwm_config.clone(),
            Tab::All => return None,
        })
    }

    /// Hand the focus back to the rest of the window (page change, click elsewhere).
    pub fn leave(&mut self) {
        self.active = false;
        self.drag = false;
    }
}

// ---------------------------------------------------------------- loading and the overview text

fn read_file(path: &Path) -> Result<String, String> {
    match std::fs::metadata(path) {
        Ok(md) if md.len() > bundle::MAX_TEXT => return Err(format!("{} is larger than 1 MB; edit it with an editor of your choice", path.display())),
        _ => {}
    }
    match std::fs::read_to_string(path) {
        Ok(t) => Ok(t),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

fn diag_lines(d: &[Diag]) -> Vec<(usize, String)> {
    d.iter().filter_map(|d| d.line.map(|l| (l.saturating_sub(1), d.msg.clone()))).collect()
}

/// Every effective setting as one TOML-looking text (defaults filled in and marked).
pub fn overview(m: &Model) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let _ = writeln!(out, "# Every effective setting, defaults included. Read only: change them in the other tabs or panes.");
    let _ = writeln!(out, "# A trailing '# default' marks a value you have not set.\n");
    let _ = writeln!(out, "## Window manager (rdwm config.toml)");
    let set = parts::flatten_toml(&m.store.text);
    let mut section = String::new();
    for k in &m.schema.keys {
        if k.section != section {
            section = k.section.clone();
            let _ = writeln!(out, "\n[{section}]");
        }
        let flat = format!("{}.{}", k.section, k.key);
        match set.get(&flat) {
            Some(v) => {
                let _ = writeln!(out, "{} = {v}", k.key);
            }
            None if k.default.is_null() => {
                let _ = writeln!(out, "# {} is not set", k.key);
            }
            None => {
                let _ = writeln!(out, "{} = {}  # default", k.key, k.default);
            }
        }
    }
    let defaults_shell = serde_json::json!({
        "theme": {"name": "macos", "mode": "dark"},
        "workspaces": "auto",
        "bar": {"enabled": true},
        "dock": {"enabled": true},
        "clockHour12": false,
        "clockDate": "weekday-day-month",
        "hot_corners": {"top_left": "none", "top_right": "none", "bottom_left": "none", "bottom_right": "none", "dwell_ms": crate::hot::DEFAULT_DWELL_MS},
    });
    let defaults_dock = serde_json::json!({"appSpacing": 6, "magnification": 1.4, "transparency": 0.4, "style": "mac", "windowsAlign": "center", "windowsLabels": "never", "windowsSearch": "box", "windowsPosition": "bottom", "windowsTheme": "auto"});
    for (title, defaults, actual) in [("Shell (decebal.json)", defaults_shell, &m.shell), ("Dock (dock-settings.json)", defaults_dock, &m.dock)] {
        let _ = writeln!(out, "\n## {title}\n");
        let (d, a) = (parts::flatten_json(&defaults), parts::flatten_json(actual));
        let mut keys: Vec<&String> = d.keys().chain(a.keys()).collect();
        keys.sort();
        keys.dedup();
        for k in keys {
            match a.get(k) {
                Some(v) => {
                    let _ = writeln!(out, "{k} = {v}");
                }
                None => {
                    let _ = writeln!(out, "{k} = {}  # default", d[k]);
                }
            }
        }
    }
    let pinned = m.pinned_list_pub();
    let _ = writeln!(out, "\n## Pinned apps (dock-pinned-macos.json)\n");
    let _ = writeln!(out, "pinned = [{}]", pinned.iter().map(|p| format!("{p:?}")).collect::<Vec<_>>().join(", "));
    out
}

fn load_buf(m: &Model, tab: Tab) -> Buf {
    let st = &m.cfg;
    match st.path_of(tab) {
        None => {
            let mut ed = Editor::new(&overview(m), tab.lang());
            ed.read_only = true;
            Buf { ed, stamp: Stamp::default(), errors: Vec::new(), disk_changed: false, note: None }
        }
        Some(path) => {
            let (text, err) = match read_file(&path) {
                Ok(t) => (t, None),
                Err(e) => (String::new(), Some(e)),
            };
            let mut ed = Editor::new(&text, tab.lang());
            ed.read_only = err.is_some();
            let errors = match (&err, tab.kind()) {
                (Some(e), _) => vec![Diag::plain(e.clone())],
                (None, Some(k)) if !text.trim().is_empty() => validate::quick(k, &text),
                _ => Vec::new(),
            };
            Buf { ed, stamp: io::stamp(&path), errors, disk_changed: false, note: None }
        }
    }
}

fn ensure_buf(m: &mut Model) {
    let tab = m.cfg.tab;
    if !m.cfg.bufs.contains_key(&tab) {
        let b = load_buf(m, tab);
        m.cfg.bufs.insert(tab, b);
    }
}

fn buf_mut(m: &mut Model) -> &mut Buf {
    ensure_buf(m);
    let tab = m.cfg.tab;
    m.cfg.bufs.get_mut(&tab).expect("loaded")
}

// ---------------------------------------------------------------- items

fn btn(id: &str, label: &str) -> Ctl {
    Ctl::new(id, Kind::Button { label: label.into(), danger: false })
}

fn danger(id: &str, label: &str) -> Ctl {
    Ctl::new(id, Kind::Button { label: label.into(), danger: true })
}

fn text(id: &str, val: &str, hint: &str) -> Ctl {
    Ctl::new(id, Kind::Text { val: val.into(), hint: hint.into() })
}

fn switcher(screen: Screen) -> Item {
    let opts = vec![("Edit files".to_string(), "edit".to_string()), ("Presets".to_string(), "presets".to_string())];
    let sel = match screen {
        Screen::Edit => 0,
        _ => 1,
    };
    Item::Card(vec![Row::new("Configuration")
        .detail("Edit the settings files as text, or keep named presets of your whole setup to switch between and share")
        .ctl(Ctl::new("cfg:view", Kind::Seg(opts, Some(sel))))])
}

pub fn items(m: &Model) -> Vec<Item> {
    // the buffers are created lazily on first draw: `items` only reads
    let st = &m.cfg;
    let mut out = vec![switcher(st.screen)];
    match st.screen {
        Screen::Edit => edit_items(m, &mut out),
        Screen::Presets => preset_items(m, &mut out),
        Screen::Save => save_items(m, &mut out),
        Screen::Import => import_items(m, &mut out),
        Screen::Apply => apply_items(m, &mut out),
    }
    out
}

fn human_size(n: usize) -> String {
    if n < 1024 {
        format!("{n} B")
    } else {
        format!("{:.1} KB", n as f64 / 1024.0)
    }
}

fn edit_items(m: &Model, out: &mut Vec<Item>) {
    let st = &m.cfg;
    let tab = st.tab;
    let modified = |t: Tab| st.bufs.get(&t).is_some_and(|b| b.ed.is_modified());
    let opts: Vec<(String, String)> = Tab::ALL.iter().map(|t| (format!("{}{}", t.title(), if modified(*t) { " •" } else { "" }), t.id().to_string())).collect();
    let idx = Tab::ALL.iter().position(|t| *t == tab);
    let path = st.path_of(tab);
    let detail = match &path {
        Some(p) => p.display().to_string(),
        None => "Generated from rdwm's key table, your files and the defaults".to_string(),
    };
    out.push(Item::Card(vec![Row::new("File").detail(detail).ctl(Ctl::new("cfg:tab", Kind::Seg(opts, idx)))]));
    let Some(buf) = st.bufs.get(&tab) else {
        out.push(Item::Note { text: "Loading...".into(), warn: false });
        return;
    };
    let errors = diag_lines(&buf.errors);
    let status = match tab {
        Tab::Rdwm | Tab::All => format!("TOML · {} lines · {}", buf.ed.line_count(), human_size(buf.ed.text().len())),
        _ => format!("JSON · {} lines · {}", buf.ed.line_count(), human_size(buf.ed.text().len())),
    };
    let mut v = buf.ed.view(EDITOR_ID, &errors, st.active, EDITOR_H, status);
    v.read_only = buf.ed.read_only;
    out.push(Item::Editor(v));
    for d in &buf.errors {
        out.push(Item::Note { text: validate::describe(d), warn: true });
    }
    if buf.disk_changed {
        out.push(Item::Note { text: "This file changed on disk while you were editing. Reload it (your edits here are discarded) or keep editing and save to overwrite it.".into(), warn: true });
    }
    if let Some(n) = &buf.note {
        out.push(Item::Note { text: n.clone(), warn: false });
    }
    let Some(p) = path else {
        out.push(Item::Note { text: "Click in the text and use Ctrl+C to copy. To change a value, open the tab of the file it lives in or use the other panes.".into(), warn: false });
        return;
    };
    let mut rows = Vec::new();
    let can_save = buf.ed.is_modified() && !buf.ed.read_only;
    let mut save = Ctl::new("cfg:save", Kind::Button { label: "Save".into(), danger: false });
    save.enabled = can_save;
    rows.push(Row::new("Save").detail("Checks the file first; a file with errors is never written. Keeps a timestamped backup. Ctrl+S").ctl(save));
    if buf.ed.is_modified() {
        rows.push(Row::new("Discard changes").detail("Go back to the text on disk").ctl(danger("cfg:discard", "Discard")));
    }
    if buf.disk_changed || !buf.ed.is_modified() {
        rows.push(Row::new("Reload from disk").detail("Read the file again").ctl(btn("cfg:reload", "Reload")));
    }
    let backups = fsx::backups(&p);
    if let Some(b) = backups.first() {
        rows.push(
            Row::new("Revert to the previous version")
                .detail(format!("Puts back {}; the current version is kept as a backup too", b.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()))
                .ctl(danger("cfg:revert", "Revert")),
        );
    }
    if tab.lang() == Lang::Json && !buf.ed.read_only {
        rows.push(Row::new("Tidy up").detail("Re-indent the JSON (one undo step)").ctl(btn("cfg:format", "Format")));
    }
    out.push(Item::Card(rows));
    let live = if tab == Tab::Rdwm { "rdwm reloads right after a save." } else { "Running decbar and decdock follow within half a second." };
    out.push(Item::Note { text: format!("Click into the text to type; Esc leaves it. Ctrl+Z undo, Ctrl+Shift+Z redo, Ctrl+A select all, Ctrl+C / X / V clipboard, Tab indents. {live}"), warn: false });
}

fn origin_tag(p: &Preset) -> &'static str {
    match p.manifest.origin {
        Origin::Builtin => "built-in",
        Origin::Imported => "imported",
        Origin::Backup => "restore point",
        Origin::Local => "yours",
    }
}

fn preset_row(p: &Preset, selected: bool) -> Row {
    let parts = p.manifest.parts.0.iter().map(|x| x.title()).collect::<Vec<_>>().join(", ");
    let detail = if p.manifest.description.is_empty() { parts } else { format!("{} ({parts})", p.manifest.description.lines().next().unwrap_or("")) };
    let mut r = Row::new(format!("{}  ·  {}", p.manifest.name, origin_tag(p))).detail(detail).ctl(btn(&format!("cfg:details:{}", p.id), "Details")).ctl(btn(&format!("cfg:apply:{}", p.id), "Apply"));
    r.selected = selected;
    r
}

fn preset_items(m: &Model, out: &mut Vec<Item>) {
    let st = &m.cfg;
    let Some((all, bad)) = st.listing.as_ref() else {
        out.push(Item::Note { text: "Loading...".into(), warn: false });
        return;
    };
    out.push(Item::Card(vec![
        Row::new("Save the current setup as a preset").detail("Pick what to include: theme, dock, window manager, shortcuts, hot corners, fonts, wallpaper").ctl(btn("cfg:new", "Save as preset...")),
        Row::new("Import a preset").detail("From a .decebal-preset file or from text you copied; you see what it contains and what it would run first").ctl(btn("cfg:import", "Import...")),
    ]));
    if let Some((name, _)) = &st.last_applied {
        out.push(Item::Card(vec![Row::new(format!("Applied \"{name}\"")).detail("Not what you wanted? Put everything back as it was before.").ctl(btn("cfg:undo", "Undo"))]));
    }
    let sel = st.selected.as_deref();
    for (title, filter) in
        [("Built-in presets (read only)", Origin::Builtin), ("Your presets", Origin::Local), ("Imported", Origin::Imported), ("Restore points (made automatically before an apply)", Origin::Backup)]
    {
        let rows: Vec<Row> = all.iter().filter(|p| p.manifest.origin == filter).map(|p| preset_row(p, sel == Some(p.id.as_str()))).collect();
        if !rows.is_empty() {
            out.push(Item::Header(title.into()));
            out.push(Item::Card(rows));
        }
    }
    if !bad.is_empty() {
        out.push(Item::Note { text: format!("Ignored (not valid presets): {}", bad.iter().map(|(n, e)| format!("{n}: {e}")).collect::<Vec<_>>().join("; ")), warn: true });
    }
    if let Some(p) = sel.and_then(|id| all.iter().find(|p| p.id == id)) {
        out.push(Item::Header(format!("Selected: {}", p.manifest.name)));
        let mut rows = vec![Row::new("Contains").detail(p.manifest.parts.0.iter().map(|x| x.title()).collect::<Vec<_>>().join(", "))];
        let files: Vec<String> = p.bundle_files().iter().map(|(n, d)| format!("{n} ({})", human_size(d.len()))).collect();
        rows.push(Row::new("Files").detail(files.join(", ")));
        rows.push(Row::new("Mode").detail(if p.manifest.merge { "Changes only the keys it mentions" } else { "Replaces everything in the parts it contains" }));
        if p.manifest.commands_stripped {
            rows.push(Row::new("Commands").detail("Removed when it was imported"));
        }
        let mut acts = vec![Row::new("Apply").detail("Shows what changes first; saves a restore point so you can undo").ctl(btn("cfg:apply-sel", "Apply..."))];
        acts.push(Row::new("Duplicate").detail("An editable copy").ctl(btn("cfg:dup", "Duplicate")));
        if !p.is_builtin() {
            acts.push(Row::new("Rename").ctl(btn("cfg:rename", "Rename")).ctl(text("cfg:rename-text", &st.rename_text, p.manifest.name.as_str())));
            acts.push(Row::new("Delete").detail("Removes the preset folder").ctl(danger("cfg:delete", "Delete")));
        }
        acts.push(Row::new("Export to a file").detail("A single shareable .decebal-preset").ctl(btn("cfg:export-browse", "Browse...")).ctl(btn("cfg:export", "Export")).ctl(text(
            "cfg:export-path",
            &st.export_path,
            "folder or file name",
        )));
        acts.push(Row::new("Copy to the clipboard").detail("As text you can paste into a message").ctl(btn("cfg:copy", "Copy")));
        out.push(Item::Card(rows));
        out.push(Item::Card(acts));
        let cmds = p.commands();
        if !cmds.is_empty() {
            out.push(Item::Note {
                text: format!(
                    "This preset contains {} command(s) that run on your desktop:\n{}",
                    cmds.len(),
                    cmds.iter().map(|c| format!("  {}   ({})", c.command, c.source)).collect::<Vec<_>>().join("\n")
                ),
                warn: true,
            });
        }
    }
    out.push(Item::Note { text: format!("Presets are folders in {}. They can be shared as one file (Export). Command line: decthemectl profile --help.", st.env.presets_dir.display()), warn: false });
}

fn has_icons(shell: &serde_json::Value) -> bool {
    ["icons", "iconTheme", "icon_theme", "icon-theme", "iconPack"].iter().any(|k| shell.get(*k).is_some())
}

fn save_items(m: &Model, out: &mut Vec<Item>) {
    let f = &m.cfg.form;
    out.push(Item::Header("Save the current setup as a preset".into()));
    out.push(Item::Card(vec![
        Row::new("Name").ctl(text("cfg:form-name", &f.name, "My setup")),
        Row::new("Description").detail("Shown in the list and to whoever you share it with").ctl(text("cfg:form-desc", &f.description, "optional")),
        Row::new("Save").detail("Pick what to include below first if you like").ctl(btn("cfg:form-save", "Save preset")).ctl(btn("cfg:form-cancel", "Cancel")),
    ]));
    out.push(Item::Header("What to include".into()));
    let has_icons = has_icons(&m.shell);
    let rows: Vec<Row> = Part::ALL
        .into_iter()
        .filter(|p| *p != Part::Icons || has_icons)
        .map(|p| Row::new(p.title()).detail(p.detail()).ctl(Ctl::new(format!("cfg:part:{}", p.id()), Kind::Toggle(f.parts.has(p)))))
        .collect();
    out.push(Item::Card(rows));
    let more = vec![Row::new("Embed the wallpaper image").detail("Puts the picture inside the preset so it travels with it (up to 24 MB)").ctl(Ctl::new("cfg:embed", Kind::Toggle(f.embed)))];
    out.push(Item::Card(more));
    if f.parts.has(Part::Autostart) || f.parts.has(Part::Keys) || f.parts.has(Part::HotCorners) {
        out.push(Item::Note { text: "Autostart entries, plugins, scratchpads, shortcuts that start programs and custom hot corners run commands. Whoever imports the preset is shown the full list before anything happens, but think twice before sharing yours.".into(), warn: false });
    }
}

fn plan_items(plan: &Plan, out: &mut Vec<Item>) {
    for e in &plan.errors {
        out.push(Item::Note { text: e.clone(), warn: true });
    }
    let rows: Vec<Row> = plan
        .changes
        .iter()
        .map(|c| {
            let what = match c.status {
                Status::New => "will be created".to_string(),
                Status::Changed => format!("{} setting(s) differ", c.total.max(1)),
                Status::Same => "already the same".to_string(),
            };
            Row::new(c.label.clone()).detail(format!("{what} · {}", c.path.display()))
        })
        .collect();
    if !rows.is_empty() {
        out.push(Item::Header("Files".into()));
        out.push(Item::Card(rows));
    }
    for c in plan.changes.iter().filter(|c| !c.lines.is_empty()) {
        let mut t = format!("{}\n{}", c.label, c.lines.join("\n"));
        if c.total > c.lines.len() {
            t.push_str(&format!("\n... and {} more", c.total - c.lines.len()));
        }
        out.push(Item::Note { text: t, warn: false });
    }
    for n in &plan.notes {
        out.push(Item::Note { text: format!("Note: {n}"), warn: false });
    }
}

fn commands_note(plan: &Plan, out: &mut Vec<Item>) {
    if plan.commands.is_empty() {
        return;
    }
    let list = plan.commands.iter().map(|c| format!("  {}   ({})", c.command, c.source)).collect::<Vec<_>>().join("\n");
    let new = plan.new_commands.len();
    out.push(Item::Note {
        text: format!(
            "WARNING: this preset would make your desktop run {} command(s), {new} of them new. rdwm starts autostart entries and plugins by itself and runs shortcut commands whenever the key is pressed, with your full user rights:\n{list}",
            plan.commands.len()
        ),
        warn: true,
    });
}

fn import_items(m: &Model, out: &mut Vec<Item>) {
    let st = &m.cfg;
    out.push(Item::Header("Import a preset".into()));
    out.push(Item::Card(vec![
        Row::new("File").detail("A .decebal-preset file").ctl(btn("cfg:import-browse", "Browse...")).ctl(btn("cfg:import-read", "Read")).ctl(text(
            "cfg:import-path",
            &st.import_path,
            "/path/to/file.decebal-preset",
        )),
        Row::new("Clipboard").detail("Text that starts with DECEBAL-PRESET-1:").ctl(btn("cfg:import-paste", "Paste from clipboard")),
    ]));
    if let Some(imp) = &st.import {
        let p = &imp.preset;
        out.push(Item::Header(format!("Preview: {}", p.manifest.name)));
        let mut rows = vec![Row::new(p.manifest.name.clone()).detail(if p.manifest.description.is_empty() { "No description".to_string() } else { p.manifest.description.clone() })];
        rows.push(Row::new("From").detail(if p.manifest.author.is_empty() { imp.source.clone() } else { format!("{} · {}", p.manifest.author, imp.source) }));
        rows.push(Row::new("Contains").detail(p.manifest.parts.0.iter().map(|x| x.title()).collect::<Vec<_>>().join(", ")));
        rows.push(Row::new("Files").detail(p.bundle_files().iter().map(|(n, d)| format!("{n} ({})", human_size(d.len()))).collect::<Vec<_>>().join(", ")));
        out.push(Item::Card(rows));
        plan_items(&imp.plan, out);
        commands_note(&imp.plan, out);
        let mut rows = Vec::new();
        if !p.commands().is_empty() {
            rows.push(Row::new("Let these commands run").detail("Off: they are removed from the imported copy").ctl(Ctl::new("cfg:keepcmds", Kind::Toggle(imp.keep_commands))));
        }
        rows.push(
            Row::new("Import")
                .detail("Adds it to your presets; nothing is applied yet")
                .ctl(btn("cfg:import-do", "Import"))
                .ctl(btn("cfg:import-apply", "Import and apply"))
                .ctl(btn("cfg:import-cancel", "Cancel")),
        );
        out.push(Item::Card(rows));
    }
}

fn apply_items(m: &Model, out: &mut Vec<Item>) {
    let Some(a) = &m.cfg.apply else { return };
    out.push(Item::Header(format!("Apply \"{}\"", a.preset.manifest.name)));
    out.push(Item::Note {
        text: if a.preset.manifest.merge {
            "This preset changes only the settings it lists. Everything is checked first and a restore point is saved so you can undo.".into()
        } else {
            "This preset replaces the parts it contains. Everything is checked first and a restore point is saved so you can undo.".into()
        },
        warn: false,
    });
    plan_items(&a.plan, out);
    commands_note(&a.plan, out);
    let mut apply =
        Ctl::new("cfg:apply-do", Kind::Button { label: if a.plan.commands.is_empty() { "Apply".into() } else { "Apply, I trust these commands".into() }, danger: !a.plan.commands.is_empty() });
    apply.enabled = a.plan.errors.is_empty();
    out.push(Item::Card(vec![Row::new(if a.plan.changed() { "Apply now" } else { "Nothing would change" }).ctl(apply).ctl(btn("cfg:apply-cancel", "Cancel"))]));
}

// ---------------------------------------------------------------- called before the pane is drawn

pub fn prepare(m: &mut Model) {
    match m.cfg.screen {
        Screen::Edit => ensure_buf(m),
        Screen::Presets => {
            m.cfg.presets();
        }
        _ => {}
    }
}

/// After a frame: take over the scroll position and page size the painter settled on.
pub fn sync_geometry(m: &mut Model, frame: &Frame) {
    if m.cfg.screen != Screen::Edit {
        return;
    }
    if let Some((_, g)) = frame.editors.first() {
        let g = *g;
        buf_mut(m).ed.sync(&g);
    }
}

// ---------------------------------------------------------------- tick

/// Disk changes, picker and clipboard results. True when the screen needs a repaint.
pub fn poll(m: &mut Model) -> bool {
    let mut redraw = false;
    loop {
        let ev = m.cfg.events.lock().unwrap().pop_front();
        let Some(ev) = ev else { break };
        redraw = true;
        match ev {
            Ev::Picked(Purpose::Import, Some(p)) => {
                m.cfg.import_path = p.display().to_string();
                read_import_file(m);
            }
            Ev::Picked(Purpose::Export, Some(p)) => {
                m.cfg.export_path = p.display().to_string();
                export_file(m);
            }
            Ev::Picked(_, None) => {}
            Ev::Clip(Ok(t)) => import_text(m, &t),
            Ev::Clip(Err(e)) => m.say(format!("Nothing to paste: {e}"), true),
        }
    }
    // files edited elsewhere
    let tabs: Vec<Tab> = m.cfg.bufs.keys().copied().collect();
    for tab in tabs {
        let Some(path) = m.cfg.path_of(tab) else {
            if tab == Tab::All && m.cfg.screen == Screen::Edit && tab == m.cfg.tab && !m.dirty {
                continue;
            }
            continue;
        };
        let now = io::stamp(&path);
        let (modified, changed) = {
            let b = &m.cfg.bufs[&tab];
            (b.ed.is_modified(), b.stamp != now)
        };
        if !changed {
            continue;
        }
        if modified {
            if let Some(b) = m.cfg.bufs.get_mut(&tab) {
                b.disk_changed = true;
            }
        } else {
            let fresh = load_buf(m, tab);
            m.cfg.bufs.insert(tab, fresh);
        }
        redraw = true;
    }
    // the overview follows the other files
    if m.cfg.bufs.contains_key(&Tab::All) && m.cfg.tab == Tab::All {
        let text = overview(m);
        if let Some(b) = m.cfg.bufs.get_mut(&Tab::All) {
            if b.ed.text() != text {
                let (top, cur) = (b.ed.top, b.ed.cursor);
                b.ed.set_text(&text);
                b.ed.top = top;
                b.ed.cursor = cur;
                b.ed.reveal = false;
                redraw = true;
            }
        }
    }
    redraw
}

// ---------------------------------------------------------------- keyboard and pointer for the editor

pub(crate) fn system_clipboard() -> Option<String> {
    use std::process::{Command, Stdio};
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some() && std::env::var("DECEBAL_BACKEND").as_deref() != Ok("x11");
    let mut tries: Vec<(&str, Vec<&str>)> = Vec::new();
    if wayland {
        tries.push(("wl-paste", vec!["-n"]));
    }
    if std::env::var_os("DISPLAY").is_some() {
        tries.push(("xclip", vec!["-selection", "clipboard", "-o"]));
    }
    for (prog, args) in tries {
        let Ok(mut child) = Command::new(prog).args(&args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn() else { continue };
        let mut out = child.stdout.take()?;
        let reader = std::thread::spawn(move || {
            let mut s = Vec::new();
            let _ = std::io::Read::read_to_end(&mut out, &mut s);
            s
        });
        let start = std::time::Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(st)) => {
                    let data = reader.join().ok()?;
                    if st.success() {
                        return String::from_utf8(data).ok().filter(|s| s.len() <= 4 * 1024 * 1024);
                    }
                    break;
                }
                Ok(None) if start.elapsed() < std::time::Duration::from_millis(700) => std::thread::sleep(std::time::Duration::from_millis(5)),
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break;
                }
            }
        }
    }
    None
}

fn recheck(m: &mut Model) {
    let tab = m.cfg.tab;
    if let Some(kind) = tab.kind() {
        let b = buf_mut(m);
        let text = b.ed.text();
        b.errors = if text.trim().is_empty() { Vec::new() } else { validate::quick(kind, &text) };
        b.note = None;
    }
}

/// Accessibility: edits the open buffer the way typing would (same buffer, undo history and re-check). Offsets are
/// characters of the text joined with `\n`. Saving stays an explicit action (the Save button or Ctrl+S).
pub fn a11y_edit(m: &mut Model, op: &crate::a11y::Op) -> Result<(), String> {
    use crate::a11y::Op;
    if m.cfg.screen != Screen::Edit {
        return Err("the editor is not open".into());
    }
    let b = buf_mut(m);
    let e = &mut b.ed;
    let mutates = !matches!(op, Op::SetCaret(_) | Op::SetSelection(_) | Op::CopyText { .. } | Op::Focus | Op::ScrollTo);
    if mutates && e.read_only {
        return Err("this file is read-only here".into());
    }
    let pos = |e: &super::editor::Editor, off: usize| {
        let mut left = off;
        for i in 0..e.line_count() {
            let n = e.line(i).chars().count();
            if left <= n {
                return super::editor::Pos::new(i, left);
            }
            left -= n + 1;
        }
        let last = e.line_count().saturating_sub(1);
        super::editor::Pos::new(last, e.line(last).chars().count())
    };
    let select = |e: &mut super::editor::Editor, a: usize, z: usize| {
        let (pa, pz) = (pos(e, a), pos(e, z));
        e.set_cursor(pa, false);
        e.set_cursor(pz, true);
    };
    match op {
        Op::SetCaret(o) => {
            let p = pos(e, *o);
            e.set_cursor(p, false);
        }
        Op::SetSelection(Some((a, z))) => select(e, *a, *z),
        Op::SetSelection(None) => {
            let c = e.cursor;
            e.set_cursor(c, false);
        }
        Op::SetText(t) => {
            e.select_all();
            e.insert_str(t);
        }
        Op::InsertText { offset, text } => {
            let p = pos(e, *offset);
            e.set_cursor(p, false);
            e.insert_str(text);
        }
        Op::DeleteText { start, end, cut } => {
            if start == end {
                return Ok(());
            }
            select(e, *start, *end);
            if *cut {
                if let Some(t) = e.cut() {
                    e.clip = t.clone();
                    crate::clipboard::copy(&t);
                }
            } else {
                e.backspace();
            }
        }
        Op::CopyText { start, end } => {
            select(e, *start, *end);
            if let Some(t) = e.selected_text() {
                e.clip = t.clone();
                crate::clipboard::copy(&t);
            }
        }
        Op::PasteText { offset } => {
            let p = pos(e, *offset);
            e.set_cursor(p, false);
            let t = system_clipboard().filter(|t| !t.is_empty()).unwrap_or_else(|| e.clip.clone());
            e.paste(&t);
        }
        _ => return Err("not a text operation".into()),
    }
    if mutates {
        recheck(m);
    }
    Ok(())
}

/// A key while the editor has the focus. Always handled (Esc leaves the editor).
pub fn editor_key(m: &mut Model, ev: &KeyEvent) -> Response {
    ensure_buf(m);
    let ctrl = ev.ctrl;
    let shift = ev.shift;
    let ch = ev.ch;
    if ev.key == Key::Escape {
        m.cfg.leave();
        return Response::REDRAW;
    }
    if ctrl && ev.key == Key::Char && ch == Some('s') {
        save_tab(m);
        return Response::REDRAW;
    }
    {
        let tab = m.cfg.tab;
        let b = m.cfg.bufs.get_mut(&tab).expect("loaded");
        let e = &mut b.ed;
        match ev.key {
            Key::Left if ctrl => e.word_left(shift),
            Key::Right if ctrl => e.word_right(shift),
            Key::Left => e.left(shift),
            Key::Right => e.right(shift),
            Key::Up => e.up(shift),
            Key::Down => e.down(shift),
            Key::Home if ctrl => e.doc_start(shift),
            Key::End if ctrl => e.doc_end(shift),
            Key::Home => e.home(shift),
            Key::End => e.end(shift),
            Key::PageUp => e.page_up(shift),
            Key::PageDown => e.page_down(shift),
            Key::Enter => e.newline(),
            Key::Backspace if ctrl => e.delete_word_back(),
            Key::Backspace => e.backspace(),
            Key::Delete if ctrl => e.delete_word_fwd(),
            Key::Delete => e.delete(),
            Key::Tab if shift => e.dedent(),
            Key::Tab => e.indent(),
            Key::Char if ctrl && !ev.alt => match ch {
                Some('a') => e.select_all(),
                Some('z') if shift => {
                    e.redo();
                }
                Some('z') => {
                    e.undo();
                }
                Some('y') => {
                    e.redo();
                }
                Some('c') => {
                    if let Some(t) = e.selected_text() {
                        e.clip = t.clone();
                        crate::clipboard::copy(&t);
                    }
                }
                Some('x') => {
                    if let Some(t) = e.cut() {
                        e.clip = t.clone();
                        crate::clipboard::copy(&t);
                    }
                }
                Some('v') => {
                    let t = system_clipboard().filter(|t| !t.is_empty()).unwrap_or_else(|| e.clip.clone());
                    e.paste(&t);
                }
                Some('k') => e.kill_line(),
                _ => {}
            },
            Key::Char if !ctrl && !ev.alt => e.insert_str(&ev.text),
            _ => {}
        }
    }
    recheck(m);
    Response::REDRAW
}

fn editor_geo(frame: &Frame) -> Option<(super::ui::Rect, Geo)> {
    frame.editors.first().copied()
}

/// Mouse press. True when the click belonged to the editor (or ended its focus).
pub fn pointer_press(m: &mut Model, frame: &Frame, p: &Painter, x: f32, y: f32) -> bool {
    if m.cfg.screen != Screen::Edit {
        return false;
    }
    let Some((rect, g)) = editor_geo(frame) else { return false };
    if !super::ui::inside(rect, x, y) {
        m.cfg.leave();
        return false;
    }
    let now = std::time::Instant::now();
    let clicks = match m.cfg.last_click {
        Some((t, lx, ly, n)) if now.duration_since(t) < std::time::Duration::from_millis(450) && (lx - x).abs() < 4.0 && (ly - y).abs() < 4.0 => n % 3 + 1,
        _ => 1,
    };
    m.cfg.last_click = Some((now, x, y, clicks));
    ensure_buf(m);
    m.cfg.active = true;
    let tab = m.cfg.tab;
    let b = m.cfg.bufs.get_mut(&tab).expect("loaded");
    // clicks on the status strip or the gutter only focus
    let in_text = y >= g.text.1 && y < g.text.1 + g.text.3;
    if !in_text {
        return true;
    }
    let pos = b.ed.hit(p, &g, x, y);
    b.ed.set_cursor(pos, false);
    b.ed.reveal = false;
    match clicks {
        2 => b.ed.select_word(),
        3 => b.ed.select_line(),
        _ => m.cfg.drag = true,
    }
    true
}

pub fn pointer_drag(m: &mut Model, frame: &Frame, p: &Painter, x: f32, y: f32) -> bool {
    if !m.cfg.drag {
        return false;
    }
    let Some((_, g)) = editor_geo(frame) else { return false };
    ensure_buf(m);
    let tab = m.cfg.tab;
    let b = m.cfg.bufs.get_mut(&tab).expect("loaded");
    let mut pos = b.ed.hit(p, &g, x, y);
    // dragging above or below the text scrolls
    if y < g.text.1 {
        b.ed.top = b.ed.top.saturating_sub(1);
        pos.line = b.ed.top;
    } else if y > g.text.1 + g.text.3 {
        b.ed.top = (b.ed.top + 1).min(b.ed.line_count().saturating_sub(g.page));
        pos.line = (b.ed.top + g.page).min(b.ed.line_count() - 1);
        pos.col = pos.col.min(b.ed.line(pos.line).chars().count());
    }
    b.ed.set_cursor(pos, true);
    b.ed.reveal = false;
    true
}

pub fn pointer_release(m: &mut Model) {
    m.cfg.drag = false;
}

/// Wheel over the editor. `dy` > 0 scrolls down. True when handled.
pub fn wheel(m: &mut Model, frame: &Frame, x: f32, y: f32, dy: f32) -> bool {
    if m.cfg.screen != Screen::Edit {
        return false;
    }
    let Some((rect, g)) = editor_geo(frame) else { return false };
    if !super::ui::inside(rect, x, y) {
        return false;
    }
    let b = buf_mut(m);
    let max = b.ed.line_count().saturating_sub(g.page);
    if max == 0 {
        return false;
    }
    let d = (dy / 30.0).round() as isize;
    let d = if d == 0 { dy.signum() as isize } else { d }.clamp(-8, 8);
    let top = (g.top as isize + d * 3).clamp(0, max as isize) as usize;
    b.ed.top = top;
    b.ed.reveal = false;
    true
}

// ---------------------------------------------------------------- actions

fn open_tab(m: &mut Model, tab: Tab) {
    m.cfg.tab = tab;
    ensure_buf(m);
}

fn save_tab(m: &mut Model) {
    let tab = m.cfg.tab;
    let (Some(kind), Some(path)) = (tab.kind(), m.cfg.path_of(tab)) else { return };
    ensure_buf(m);
    let (text, modified, changed_on_disk, read_only) = {
        let b = &m.cfg.bufs[&tab];
        (b.ed.text(), b.ed.is_modified(), b.stamp != io::stamp(&path), b.ed.read_only)
    };
    if read_only {
        return m.say("This file cannot be edited here", true);
    }
    if !modified {
        return m.say("Nothing to save", false);
    }
    if changed_on_disk && !m.cfg.bufs[&tab].disk_changed {
        if let Some(b) = m.cfg.bufs.get_mut(&tab) {
            b.disk_changed = true;
        }
    }
    let diags = validate::validate(kind, &text, &m.cfg.env.rdwmctl, &path);
    if !diags.is_empty() {
        let first = validate::describe(&diags[0]);
        if let Some(b) = m.cfg.bufs.get_mut(&tab) {
            b.errors = diags;
            b.note = None;
        }
        // jump to the first marked line
        if let Some(l) = m.cfg.bufs[&tab].errors.iter().find_map(|d| d.line) {
            let b = m.cfg.bufs.get_mut(&tab).expect("loaded");
            b.ed.set_cursor(Pos::new(l.saturating_sub(1), 0), false);
        }
        return m.say(format!("Not saved: {first}"), true);
    }
    match fsx::save_with_backup(&path, &text) {
        Ok(bak) => {
            let stamp = io::stamp(&path);
            if let Some(b) = m.cfg.bufs.get_mut(&tab) {
                b.ed.mark_saved();
                b.stamp = stamp;
                b.errors.clear();
                b.disk_changed = false;
                b.note = Some(match &bak {
                    Some(p) => format!("Saved. The previous version is kept as {}.", p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
                    None => "Saved.".to_string(),
                });
            }
            applied_live(m, tab);
            m.say("Saved", false);
        }
        Err(e) => m.say(format!("Not saved: {e}"), true),
    }
}

/// After a file was written: tell the running programs.
fn applied_live(m: &mut Model, tab: Tab) {
    m.on_files_changed();
    if tab == Tab::Rdwm {
        m.reload_rdwm();
    }
    // the bar, dock and theme watchers pick the JSON files up on their own
}

fn reload_tab(m: &mut Model) {
    let tab = m.cfg.tab;
    let fresh = load_buf(m, tab);
    m.cfg.bufs.insert(tab, fresh);
}

fn revert_tab(m: &mut Model) {
    let tab = m.cfg.tab;
    let Some(path) = m.cfg.path_of(tab) else { return };
    if let Some(kind) = tab.kind() {
        let _ = kind;
    }
    match fsx::revert_latest(&path) {
        Ok(from) => {
            reload_tab(m);
            applied_live(m, tab);
            m.say(format!("Reverted to {}", from.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()), false);
        }
        Err(e) => m.say(format!("Not reverted: {e}"), true),
    }
}

fn format_tab(m: &mut Model) {
    let b = buf_mut(m);
    match serde_json::from_str::<serde_json::Value>(&b.ed.text()) {
        Ok(v) => {
            let t = parts::pretty_json(&v);
            if t != b.ed.text() {
                b.ed.replace_all(&t);
            }
            recheck(m);
        }
        Err(e) => m.say(format!("Cannot format: {e}"), true),
    }
}

fn start_picker(m: &mut Model, purpose: Purpose) {
    if !crate::hot::in_path("zenity") {
        return m.say("No file dialog is installed (zenity); type the path instead", true);
    }
    let (events, wake) = (m.cfg.events.clone(), m.cfg.wake.clone());
    let start = match purpose {
        Purpose::Import => m.cfg.import_path.clone(),
        Purpose::Export => m.cfg.export_path.clone(),
    };
    std::thread::spawn(move || {
        let mut c = std::process::Command::new("zenity");
        c.arg("--file-selection").arg(format!("--filename={start}")).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        match purpose {
            Purpose::Import => {
                c.arg("--title=Import a preset").arg("--file-filter=Presets | *.decebal-preset").arg("--file-filter=All files | *");
            }
            Purpose::Export => {
                c.arg("--save").arg("--confirm-overwrite").arg("--title=Export the preset");
            }
        }
        let path = c.output().ok().filter(|o| o.status.success()).map(|o| PathBuf::from(String::from_utf8_lossy(&o.stdout).trim().to_string())).filter(|p| !p.as_os_str().is_empty());
        events.lock().unwrap().push_back(Ev::Picked(purpose, path));
        wake();
    });
}

fn expand(p: &str) -> PathBuf {
    PathBuf::from(super::panes::expand_tilde(p.trim()))
}

fn read_import_file(m: &mut Model) {
    let path = expand(&m.cfg.import_path);
    if m.cfg.import_path.trim().is_empty() {
        return m.say("Type or browse to a .decebal-preset file", true);
    }
    match store::read_bundle_file(&path) {
        Ok(p) => begin_import(m, p, path.display().to_string()),
        Err(e) => m.say(format!("Cannot import: {e}"), true),
    }
}

fn import_text(m: &mut Model, text: &str) {
    match bundle::from_text(text).and_then(|b| Preset::from_bundle(&b)) {
        Ok(p) => begin_import(m, p, "clipboard".into()),
        Err(e) => m.say(format!("Cannot import: {e}"), true),
    }
}

fn begin_import(m: &mut Model, preset: Preset, source: String) {
    let plan = store::plan(&m.cfg.env, &preset);
    m.cfg.import = Some(ImportState { preset, plan, keep_commands: false, source });
    m.cfg.screen = Screen::Import;
}

fn finish_import(m: &mut Model, then_apply: bool) {
    let Some(imp) = m.cfg.import.take() else { return };
    let strip = !imp.keep_commands && !imp.preset.commands().is_empty();
    match store::install(&m.cfg.env, imp.preset, strip) {
        Ok(p) => {
            m.cfg.forget_listing();
            m.cfg.selected = Some(p.id.clone());
            m.say(format!("Imported \"{}\"{}", p.manifest.name, if strip { " without its commands" } else { "" }), false);
            if then_apply {
                begin_apply(m, &p.id.clone());
            } else {
                m.cfg.screen = Screen::Presets;
            }
        }
        Err(e) => {
            m.cfg.screen = Screen::Import;
            m.say(format!("Not imported: {e}"), true);
        }
    }
}

fn begin_apply(m: &mut Model, id: &str) {
    m.cfg.forget_listing();
    let Some(preset) = m.cfg.presets().0.iter().find(|p| p.id == id).cloned() else { return m.say("That preset is gone", true) };
    let plan = store::plan(&m.cfg.env, &preset);
    m.cfg.apply = Some(ApplyState { preset, plan });
    m.cfg.screen = Screen::Apply;
}

fn do_apply(m: &mut Model) {
    let Some(a) = m.cfg.apply.take() else { return };
    match store::apply(&m.cfg.env, &a.preset, !a.preset.is_backup()) {
        Ok(r) => {
            m.cfg.forget_listing();
            m.cfg.last_applied = r.restore_point.clone().map(|id| (a.preset.manifest.name.clone(), id));
            m.cfg.screen = Screen::Presets;
            for b in m.cfg.bufs.values_mut() {
                b.stamp = Stamp::default(); // forces a reload of unmodified buffers
            }
            m.on_files_changed();
            if r.rdwm_changed {
                m.reload_rdwm();
            }
            let w = if r.warnings.is_empty() { String::new() } else { format!(" ({})", r.warnings.join("; ")) };
            m.say(format!("Applied \"{}\"{w}", a.preset.manifest.name), false);
        }
        Err(e) => {
            m.cfg.apply = Some(a);
            m.say(format!("Not applied: {e}"), true);
        }
    }
}

fn export_file(m: &mut Model) {
    let Some(p) = m.cfg.selected_preset() else { return };
    let mut path = expand(&m.cfg.export_path);
    if path.is_dir() || m.cfg.export_path.ends_with('/') {
        path = path.join(format!("{}.{}", manifest::slug(&p.manifest.name), crate::preset::EXT));
    } else if path.extension().is_none_or(|e| e != crate::preset::EXT) {
        let mut s = path.into_os_string();
        s.push(format!(".{}", crate::preset::EXT));
        path = PathBuf::from(s);
    }
    if path.exists() && !path.is_file() {
        return m.say("That is not a file", true);
    }
    match store::export_to(&p, &path) {
        Ok(n) => m.say(format!("Exported {} ({})", path.display(), human_size(n)), false),
        Err(e) => m.say(format!("Not exported: {e}"), true),
    }
}

fn copy_preset(m: &mut Model) {
    let Some(p) = m.cfg.selected_preset() else { return };
    match p.to_bundle() {
        Ok(b) if b.len() <= bundle::MAX_CLIPBOARD => {
            let ok = crate::clipboard::copy(&bundle::to_text(&b));
            m.say(if ok { "Copied; paste it into a message or into Import > Paste from clipboard" } else { "Could not reach the clipboard (install wl-clipboard or xclip)" }, !ok);
        }
        Ok(_) => m.say("Too large for the clipboard (it has a wallpaper); use Export to a file", true),
        Err(e) => m.say(format!("Cannot export: {e}"), true),
    }
}

fn save_form(m: &mut Model) {
    let f = &m.cfg.form;
    let opts = CaptureOpts { name: f.name.trim().to_string(), description: f.description.clone(), author: String::new(), parts: f.parts.clone(), embed_wallpaper: f.embed };
    if let Err(e) = manifest::check_name(&opts.name) {
        return m.say(e, true);
    }
    if m.cfg.env.presets_dir.join(manifest::slug(&opts.name)).exists() {
        return m.say(format!("A preset called \"{}\" already exists", opts.name), true);
    }
    match store::capture(&m.cfg.env, &opts).and_then(|(p, notes)| store::save(&m.cfg.env, &p, false).map(|()| (p, notes))) {
        Ok((p, notes)) => {
            m.cfg.forget_listing();
            m.cfg.selected = Some(p.id.clone());
            m.cfg.screen = Screen::Presets;
            m.say(format!("Saved preset \"{}\"{}", p.manifest.name, notes.first().map(|n| format!(" ({n})")).unwrap_or_default()), false);
        }
        Err(e) => m.say(format!("Not saved: {e}"), true),
    }
}

/// Control events of the pane (`cfg:...` ids).
pub fn apply(m: &mut Model, id: &str, v: CtlVal) {
    let Some(rest) = id.strip_prefix("cfg:") else { return };
    let click = matches!(v, CtlVal::Click);
    let s = match &v {
        CtlVal::Str(s) => s.clone(),
        _ => String::new(),
    };
    match rest {
        "view" => {
            m.cfg.leave();
            m.cfg.screen = if s == "presets" { Screen::Presets } else { Screen::Edit };
        }
        "tab" => {
            if let Some(t) = Tab::parse(&s) {
                m.cfg.leave();
                open_tab(m, t);
            }
        }
        "editor" => {
            if click {
                m.cfg.active = true;
            }
        }
        "save" => save_tab(m),
        "discard" | "reload" => {
            reload_tab(m);
            m.say(if rest == "discard" { "Changes discarded" } else { "Reloaded from disk" }, false);
        }
        "revert" => revert_tab(m),
        "format" => format_tab(m),
        "new" => {
            let mut parts = PartSet::defaults();
            // the icon theme is part of the setup as soon as the shell has one
            parts.toggle(Part::Icons, has_icons(&m.shell));
            m.cfg.form = SaveForm { name: String::new(), description: String::new(), parts, embed: false };
            m.cfg.screen = Screen::Save;
        }
        "form-name" => m.cfg.form.name = s,
        "form-desc" => m.cfg.form.description = s,
        "embed" => {
            if let CtlVal::Bool(b) = v {
                m.cfg.form.embed = b;
            }
        }
        "form-save" => save_form(m),
        "form-cancel" | "apply-cancel" | "import-cancel" => {
            m.cfg.import = None;
            m.cfg.apply = None;
            m.cfg.screen = Screen::Presets;
        }
        "import" => {
            m.cfg.import = None;
            m.cfg.screen = Screen::Import;
        }
        "import-path" => m.cfg.import_path = s,
        "import-browse" => start_picker(m, Purpose::Import),
        "import-read" => read_import_file(m),
        "import-paste" => {
            let (events, wake) = (m.cfg.events.clone(), m.cfg.wake.clone());
            std::thread::spawn(move || {
                let r = system_clipboard().ok_or_else(|| "the clipboard is empty or no clipboard tool (xclip, wl-paste) answered".to_string());
                events.lock().unwrap().push_back(Ev::Clip(r));
                wake();
            });
        }
        "keepcmds" => {
            if let (CtlVal::Bool(b), Some(i)) = (v, m.cfg.import.as_mut()) {
                i.keep_commands = b;
            }
        }
        "import-do" => finish_import(m, false),
        "import-apply" => finish_import(m, true),
        "apply-do" => do_apply(m),
        "apply-sel" => {
            if let Some(id) = m.cfg.selected.clone() {
                begin_apply(m, &id);
            }
        }
        "undo" => {
            if let Some((_, rp)) = m.cfg.last_applied.clone() {
                m.cfg.forget_listing();
                match m.cfg.presets().0.iter().find(|p| p.id == rp).cloned() {
                    Some(p) => match store::apply(&m.cfg.env, &p, false) {
                        Ok(r) => {
                            m.cfg.last_applied = None;
                            m.cfg.forget_listing();
                            m.on_files_changed();
                            if r.rdwm_changed {
                                m.reload_rdwm();
                            }
                            for b in m.cfg.bufs.values_mut() {
                                b.stamp = Stamp::default();
                            }
                            m.say("Put everything back", false);
                        }
                        Err(e) => m.say(format!("Not undone: {e}"), true),
                    },
                    None => m.say("The restore point is gone", true),
                }
            }
        }
        "dup" => {
            if let Some(p) = m.cfg.selected_preset() {
                match store::duplicate(&m.cfg.env, &p.id, None) {
                    Ok(n) => {
                        m.cfg.forget_listing();
                        m.cfg.selected = Some(n.id);
                        m.say(format!("Created \"{}\"", n.manifest.name), false);
                    }
                    Err(e) => m.say(e, true),
                }
            }
        }
        "rename-text" => m.cfg.rename_text = s,
        "rename" => {
            let name = m.cfg.rename_text.clone();
            if let Some(p) = m.cfg.selected_preset() {
                match store::rename(&m.cfg.env, &p.id, &name) {
                    Ok(n) => {
                        m.cfg.forget_listing();
                        m.cfg.selected = Some(n.id);
                        m.cfg.rename_text.clear();
                        m.say("Renamed", false);
                    }
                    Err(e) => m.say(e, true),
                }
            }
        }
        "delete" => {
            if let Some(p) = m.cfg.selected_preset() {
                match store::delete(&m.cfg.env, &p.id) {
                    Ok(()) => {
                        m.cfg.forget_listing();
                        m.cfg.selected = None;
                        m.say(format!("Deleted \"{}\"", p.manifest.name), false);
                    }
                    Err(e) => m.say(e, true),
                }
            }
        }
        "export-path" => m.cfg.export_path = s,
        "export" => export_file(m),
        "export-browse" => start_picker(m, Purpose::Export),
        "copy" => copy_preset(m),
        _ => {
            if let Some(pid) = rest.strip_prefix("part:") {
                if let (Some(p), CtlVal::Bool(b)) = (Part::parse(pid), v) {
                    m.cfg.form.parts.toggle(p, b);
                }
            } else if let Some(pid) = rest.strip_prefix("details:") {
                m.cfg.selected = Some(pid.to_string());
                m.cfg.rename_text.clear();
            } else if let Some(pid) = rest.strip_prefix("apply:") {
                m.cfg.selected = Some(pid.to_string());
                begin_apply(m, pid);
            }
        }
    }
    m.dirty = true;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::io::Paths;
    use crate::settings::schema::Schema;

    pub fn model(name: &str) -> Model {
        let d = std::env::temp_dir().join(format!("decebal-cfgpane-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("shell")).unwrap();
        let paths = Paths { rdwm_config: d.join("config.toml"), shell_dir: d.join("shell"), rdwmctl: "/nonexistent/rdwmctl".into(), socket: d.join("none.sock") };
        let queue: io::Queue = Arc::new(Mutex::new(Default::default()));
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(|| {});
        let mut m = Model::new(paths, Schema::snapshot(), queue, wake);
        m.cfg.env.presets_dir = d.join("presets");
        m.cfg.env.themes_dir = d.join("themes");
        m
    }

    fn key(k: Key, ch: Option<char>, ctrl: bool) -> KeyEvent {
        KeyEvent { key: k, text: if ctrl { String::new() } else { ch.map(String::from).unwrap_or_default() }, ch, ctrl, shift: false, alt: false }
    }

    fn type_text(m: &mut Model, s: &str) {
        for c in s.chars() {
            if c == '\n' {
                editor_key(m, &key(Key::Enter, None, false));
            } else {
                editor_key(m, &key(Key::Char, Some(c), false));
            }
        }
    }

    #[test]
    fn items_exist_for_every_screen() {
        let mut m = model("items");
        for sc in [Screen::Edit, Screen::Presets, Screen::Save] {
            m.cfg.screen = sc;
            prepare(&mut m);
            let it = items(&m);
            assert!(it.len() >= 2, "{sc:?}");
        }
        let mut ids = std::collections::HashSet::new();
        m.cfg.screen = Screen::Presets;
        prepare(&mut m);
        m.cfg.selected = Some("builtin/macos".into());
        for it in items(&m) {
            if let Item::Card(rows) = it {
                for r in rows {
                    for c in r.ctls {
                        assert!(ids.insert(c.id.clone()), "duplicate control id {}", c.id);
                    }
                }
            }
        }
        assert!(ids.contains("cfg:apply:builtin/macos") && ids.contains("cfg:apply-sel") && ids.contains("cfg:dup") && !ids.contains("cfg:delete"), "built-ins are read only: {ids:?}");
    }

    #[test]
    fn typing_saves_validates_and_reverts() {
        let mut m = model("save");
        let shell = m.paths.shell_json();
        std::fs::write(&shell, "{\n  \"workspaces\": \"auto\"\n}\n").unwrap();
        m.cfg.tab = Tab::Shell;
        prepare(&mut m);
        m.cfg.active = true;
        // go to the end of the first line and break the JSON
        editor_key(&mut m, &key(Key::Down, None, false));
        editor_key(&mut m, &key(Key::End, None, false));
        type_text(&mut m, ",\n  \"bar\": {\"enabled\": }");
        assert!(buf_mut(&mut m).ed.is_modified());
        assert!(!buf_mut(&mut m).errors.is_empty(), "live check flags the broken JSON");
        assert_eq!(buf_mut(&mut m).errors[0].line, Some(3));
        apply(&mut m, "cfg:save", CtlVal::Click);
        assert_eq!(std::fs::read_to_string(&shell).unwrap(), "{\n  \"workspaces\": \"auto\"\n}\n", "an invalid file is never written");
        assert!(m.notice.as_ref().is_some_and(|n| n.error && n.text.contains("Not saved")));
        // the failed save moved the cursor to the marked line; fix the value there
        assert_eq!(buf_mut(&mut m).ed.cursor.line, 2);
        editor_key(&mut m, &key(Key::End, None, false));
        editor_key(&mut m, &key(Key::Left, None, false));
        editor_key(&mut m, &key(Key::Left, None, false));
        type_text(&mut m, "false");
        let (errs, txt) = (buf_mut(&mut m).errors.clone(), buf_mut(&mut m).ed.text());
        assert!(errs.is_empty(), "{errs:?} {txt:?}");
        editor_key(&mut m, &key(Key::Char, Some('s'), true));
        assert!(std::fs::read_to_string(&shell).unwrap().contains("\"enabled\":false"));
        assert!(!buf_mut(&mut m).ed.is_modified());
        let baks = fsx::backups(&shell);
        assert_eq!(baks.len(), 1);
        assert_eq!(std::fs::read_to_string(&baks[0]).unwrap(), "{\n  \"workspaces\": \"auto\"\n}\n");
        // revert restores it
        apply(&mut m, "cfg:revert", CtlVal::Click);
        assert_eq!(std::fs::read_to_string(&shell).unwrap(), "{\n  \"workspaces\": \"auto\"\n}\n");
        assert_eq!(buf_mut(&mut m).ed.text(), "{\n  \"workspaces\": \"auto\"\n}\n");
        // and the model followed the file
        assert_eq!(m.shell["workspaces"], "auto");
    }

    #[test]
    fn undo_and_select_all_work_through_keys() {
        let mut m = model("undo");
        std::fs::write(m.paths.dock_settings(), "{}").unwrap();
        m.cfg.tab = Tab::Dock;
        prepare(&mut m);
        m.cfg.active = true;
        type_text(&mut m, "x");
        editor_key(&mut m, &key(Key::Char, Some('z'), true));
        assert_eq!(buf_mut(&mut m).ed.text(), "{}");
        assert!(!buf_mut(&mut m).ed.is_modified());
        editor_key(&mut m, &key(Key::Char, Some('a'), true));
        assert_eq!(buf_mut(&mut m).ed.selected_text().as_deref(), Some("{}"));
        editor_key(&mut m, &key(Key::Escape, None, false));
        assert!(!m.cfg.active);
    }

    #[test]
    fn external_edits_reload_clean_buffers_and_warn_on_dirty_ones() {
        let mut m = model("external");
        let p = m.paths.dock_settings();
        std::fs::write(&p, "{\"a\": 1}").unwrap();
        m.cfg.tab = Tab::Dock;
        prepare(&mut m);
        std::thread::sleep(std::time::Duration::from_millis(15));
        std::fs::write(&p, "{\"a\": 22}").unwrap();
        assert!(poll(&mut m));
        assert_eq!(buf_mut(&mut m).ed.text(), "{\"a\": 22}", "a clean buffer follows the file");
        m.cfg.active = true;
        type_text(&mut m, " ");
        std::thread::sleep(std::time::Duration::from_millis(15));
        std::fs::write(&p, "{\"a\": 333}").unwrap();
        poll(&mut m);
        assert!(buf_mut(&mut m).disk_changed);
        assert!(buf_mut(&mut m).ed.text().contains("22"), "unsaved edits are kept");
        let shown = items(&m);
        assert!(shown.iter().any(|i| matches!(i, Item::Note { text, warn: true } if text.contains("changed on disk"))));
    }

    #[test]
    fn all_settings_is_read_only_and_lists_defaults() {
        let mut m = model("all");
        std::fs::write(&m.paths.rdwm_config, "[general]\nborder_px = 7\n").unwrap();
        m.store = io::Store::load(&m.paths.rdwm_config, &m.paths.rdwmctl);
        m.cfg.tab = Tab::All;
        prepare(&mut m);
        let b = buf_mut(&mut m);
        assert!(b.ed.read_only);
        let t = b.ed.text();
        assert!(t.contains("border_px = 7\n"), "{t}");
        assert!(t.contains("gap_inner = 0  # default"), "defaults are filled in and marked");
        assert!(t.contains("dwell_ms") && t.contains("appSpacing = 6  # default") && t.contains("pinned = ["));
        m.cfg.active = true;
        type_text(&mut m, "zzz");
        assert_eq!(buf_mut(&mut m).ed.text(), t, "typing does nothing");
        // no save row for the overview
        let it = items(&m);
        assert!(!it.iter().any(|i| matches!(i, Item::Card(rows) if rows.iter().any(|r| r.ctls.iter().any(|c| c.id == "cfg:save")))));
    }

    #[test]
    fn rdwm_tab_reports_toml_errors_with_lines() {
        let mut m = model("rdwm");
        std::fs::write(&m.paths.rdwm_config, "[general]\nborder_px = 2\n").unwrap();
        m.cfg.tab = Tab::Rdwm;
        prepare(&mut m);
        m.cfg.active = true;
        editor_key(&mut m, &key(Key::Char, Some('a'), true));
        editor_key(&mut m, &key(Key::Delete, None, false));
        type_text(&mut m, "[general\nx = 1\n");
        let errs = buf_mut(&mut m).errors.clone();
        assert!(!errs.is_empty() && errs[0].line == Some(1), "{errs:?}");
        apply(&mut m, "cfg:save", CtlVal::Click);
        assert_eq!(std::fs::read_to_string(&m.paths.rdwm_config).unwrap(), "[general]\nborder_px = 2\n");
    }

    #[test]
    fn preset_flow_save_apply_undo_export_import() {
        let mut m = model("flow");
        std::fs::write(m.paths.shell_json(), "{\"theme\": {\"name\": \"nord\"}}").unwrap();
        std::fs::write(m.paths.dock_settings(), "{\"appSpacing\": 9}").unwrap();
        std::fs::write(&m.paths.rdwm_config, "[general]\nborder_px = 3\n[[autostart]]\ncommand = \"decbar &\"\n").unwrap();
        // save
        apply(&mut m, "cfg:view", CtlVal::Str("presets".into()));
        apply(&mut m, "cfg:new", CtlVal::Click);
        assert_eq!(m.cfg.screen, Screen::Save);
        apply(&mut m, "cfg:form-name", CtlVal::Str("Cosy".into()));
        apply(&mut m, "cfg:form-desc", CtlVal::Str("night".into()));
        apply(&mut m, "cfg:part:autostart", CtlVal::Bool(true));
        apply(&mut m, "cfg:form-save", CtlVal::Click);
        assert_eq!(m.cfg.screen, Screen::Presets, "{:?}", m.notice);
        assert_eq!(m.cfg.selected.as_deref(), Some("cosy"));
        // change things, then apply with preview
        std::fs::write(m.paths.dock_settings(), "{\"appSpacing\": 30}").unwrap();
        apply(&mut m, "cfg:apply:cosy", CtlVal::Click);
        assert_eq!(m.cfg.screen, Screen::Apply);
        let shown = items(&m);
        assert!(shown.iter().any(|i| matches!(i, Item::Note { text, warn: true } if text.contains("decbar &"))), "the commands are listed");
        apply(&mut m, "cfg:apply-do", CtlVal::Click);
        assert_eq!(m.cfg.screen, Screen::Presets, "{:?}", m.notice);
        assert!(std::fs::read_to_string(m.paths.dock_settings()).unwrap().contains("9"));
        assert!(m.cfg.last_applied.is_some());
        apply(&mut m, "cfg:undo", CtlVal::Click);
        assert!(std::fs::read_to_string(m.paths.dock_settings()).unwrap().contains("30"), "{:?}", m.notice);
        // export, then import into the same model as a second preset (stripping commands)
        let out = m.cfg.env.presets_dir.parent().unwrap().join("out");
        std::fs::create_dir_all(&out).unwrap();
        apply(&mut m, "cfg:export-path", CtlVal::Str(format!("{}/", out.display())));
        apply(&mut m, "cfg:export", CtlVal::Click);
        let file = out.join("cosy.decebal-preset");
        assert!(file.exists(), "{:?}", m.notice);
        apply(&mut m, "cfg:import", CtlVal::Click);
        apply(&mut m, "cfg:import-path", CtlVal::Str(file.display().to_string()));
        apply(&mut m, "cfg:import-read", CtlVal::Click);
        assert_eq!(m.cfg.screen, Screen::Import);
        assert!(items(&m).iter().any(|i| matches!(i, Item::Note { text, warn: true } if text.contains("WARNING"))));
        apply(&mut m, "cfg:import-do", CtlVal::Click);
        let (all, _) = store::list(&m.cfg.env);
        let imported = all.iter().find(|p| p.manifest.origin == Origin::Imported).expect("imported");
        assert!(imported.commands().is_empty(), "the safe default strips the commands");
        assert_eq!(imported.manifest.name, "Cosy (2)");
        // rename, duplicate, delete
        apply(&mut m, "cfg:rename-text", CtlVal::Str("Renamed".into()));
        apply(&mut m, "cfg:rename", CtlVal::Click);
        assert_eq!(m.cfg.selected.as_deref(), Some("renamed"));
        apply(&mut m, "cfg:dup", CtlVal::Click);
        apply(&mut m, "cfg:delete", CtlVal::Click);
        assert!(store::find(&m.cfg.env, "renamed-copy").is_none());
    }

    #[test]
    fn typing_a_whole_object_ends_without_errors() {
        let mut m = model("whole");
        std::fs::write(m.paths.shell_json(), "{\"a\": 1}").unwrap();
        m.cfg.tab = Tab::Shell;
        prepare(&mut m);
        m.cfg.active = true;
        editor_key(&mut m, &key(Key::Char, Some('a'), true));
        for line in ["{", "\"workspaces\": \"off\",", "\"clockHour12\": true", "}"] {
            type_text(&mut m, line);
            if line != "}" {
                editor_key(&mut m, &key(Key::Enter, None, false));
            }
        }
        let b = buf_mut(&mut m);
        assert!(b.errors.is_empty(), "{:?} {:?}", b.errors, b.ed.text());
    }

    #[test]
    fn colour_is_not_the_only_signal_for_the_modified_tab() {
        let mut m = model("tabs");
        std::fs::write(m.paths.shell_json(), "{}").unwrap();
        prepare(&mut m);
        m.cfg.active = true;
        type_text(&mut m, " ");
        let it = items(&m);
        let Item::Card(rows) = &it[1] else { panic!("tab card") };
        let Kind::Seg(opts, _) = &rows[0].ctls[0].kind else { panic!() };
        assert!(opts[0].0.ends_with('•'), "{opts:?}");
    }
}
