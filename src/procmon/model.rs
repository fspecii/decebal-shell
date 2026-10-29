//! The process list as a user sees it: apps (matched to `.desktop` entries) with their child processes folded in, the
//! rest as background processes, then sorting, searching and flattening into visible rows. All pure.

use super::proc::Proc;
use crate::apps::AppEntry;
use std::collections::{HashMap, HashSet};

/// Programs that are never an app by themselves (a launcher script's `Exec=sh -c ...`).
const GENERIC: [&str; 12] = ["sh", "bash", "zsh", "env", "flatpak", "python", "python3", "java", "node", "electron", "perl", "xdg-open"];

/// Interpreters whose first argument is the program that is really running.
fn is_interpreter(base: &str) -> bool {
    ["python", "perl", "ruby", "node", "java", "bash", "sh", "zsh", "lua"].iter().any(|i| base == *i || (base.starts_with(i) && base[i.len()..].chars().all(|c| c.is_ascii_digit() || c == '.')))
}

fn stem(s: &str) -> String {
    let base = s.rsplit('/').next().unwrap_or(s);
    base.strip_suffix(".desktop").unwrap_or(base).to_lowercase()
}

/// The program an `Exec=` line starts: skips `env VAR=x` and returns the lower-case base name.
pub fn exec_key(exec: &str) -> Option<String> {
    let mut toks = exec.split_whitespace().map(|t| t.trim_matches('"'));
    let mut first = toks.next()?;
    if first == "env" {
        first = toks.find(|t| !t.contains('=') && !t.starts_with('-'))?;
    }
    let k = stem(first);
    (!k.is_empty() && !k.starts_with('%') && !GENERIC.contains(&k.as_str())).then_some(k)
}

/// Installed applications, looked up by the names a running process shows.
#[derive(Default)]
pub struct AppIndex {
    pub entries: Vec<AppEntry>,
    keys: HashMap<String, usize>,
}

impl AppIndex {
    pub fn new(entries: Vec<AppEntry>) -> AppIndex {
        let mut keys = HashMap::new();
        for (i, e) in entries.iter().enumerate() {
            let mut add = |k: String| {
                if !k.is_empty() && !GENERIC.contains(&k.as_str()) {
                    keys.entry(k).or_insert(i);
                }
            };
            if let Some(k) = exec_key(&e.exec) {
                add(k);
            }
            if !e.file.is_empty() {
                add(stem(&e.file));
            }
            if !e.icon.is_empty() && !e.icon.starts_with('/') {
                add(e.icon.to_lowercase());
            }
            add(e.name.to_lowercase());
        }
        AppIndex { entries, keys }
    }

    pub fn load() -> AppIndex {
        AppIndex::new(crate::apps::list())
    }

    /// The application a process belongs to by its own names (not its parents').
    pub fn find(&self, p: &Proc) -> Option<usize> {
        if p.kthread {
            return None;
        }
        let mut names = vec![p.comm.to_lowercase()];
        if let Some(a0) = p.cmdline.first() {
            let b = stem(a0.split(' ').next().unwrap_or(a0));
            if is_interpreter(&b) {
                if let Some(a1) = p.cmdline.iter().skip(1).find(|a| !a.starts_with('-')) {
                    let s = stem(a1);
                    names.push(s.rsplit_once('.').map_or(s.clone(), |(a, _)| a.to_string()));
                    names.push(s);
                }
            }
            names.push(b);
        }
        names.iter().find_map(|n| self.keys.get(n).copied())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Apps,
    Background,
}

/// One top-level row: an app with all its processes, or a single background process.
#[derive(Debug, Clone)]
pub struct Group {
    pub section: Section,
    pub name: String,
    pub icon: Option<String>,
    /// Indices into the process list; the main process first.
    pub members: Vec<usize>,
    pub cpu: f32,
    pub rss: u64,
    pub io: f64,
}

/// Groups `procs`: every process that is (or descends from) a matched app joins that app's group; the first process of a
/// run of the same app (the topmost) is its main process. The rest are background rows of their own.
pub fn build_groups(procs: &[Proc], apps: &AppIndex) -> Vec<Group> {
    let by_pid: HashMap<u32, usize> = procs.iter().enumerate().map(|(i, p)| (p.pid, i)).collect();
    let own: Vec<Option<usize>> = procs.iter().map(|p| apps.find(p)).collect();
    // The group of a process: (app, main process).
    let root_of = |i: usize| -> Option<(usize, usize)> {
        let mut cur = i;
        let mut app = None;
        for _ in 0..64 {
            if let Some(a) = own[cur] {
                app = Some(a);
                break;
            }
            match by_pid.get(&procs[cur].ppid) {
                Some(&p) if p != cur && procs[cur].ppid > 1 => cur = p,
                _ => return None,
            }
        }
        let app = app?;
        let mut root = cur;
        for _ in 0..64 {
            match by_pid.get(&procs[root].ppid) {
                Some(&p) if p != root && own[p] == Some(app) => root = p,
                _ => break,
            }
        }
        Some((app, root))
    };
    let mut apps_groups: HashMap<usize, Group> = HashMap::new();
    let mut order: Vec<usize> = Vec::new();
    let mut out = Vec::new();
    for (i, p) in procs.iter().enumerate() {
        match root_of(i) {
            Some((app, root)) => {
                let e = &apps.entries[app];
                let g = apps_groups.entry(root).or_insert_with(|| {
                    order.push(root);
                    Group { section: Section::Apps, name: e.name.clone(), icon: (!e.icon.is_empty()).then(|| e.icon.clone()), members: vec![root], cpu: 0.0, rss: 0, io: 0.0 }
                });
                if i != root {
                    g.members.push(i);
                }
            }
            None => out.push(Group { section: Section::Background, name: p.name.clone(), icon: None, members: vec![i], cpu: 0.0, rss: 0, io: 0.0 }),
        }
    }
    out.extend(order.into_iter().filter_map(|r| apps_groups.remove(&r)));
    for g in &mut out {
        g.cpu = g.members.iter().map(|&i| procs[i].cpu).sum();
        g.rss = g.members.iter().map(|&i| procs[i].rss).sum();
        g.io = g.members.iter().map(|&i| procs[i].io_total()).sum();
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Name,
    Pid,
    Cpu,
    Mem,
    Io,
}

impl SortKey {
    /// Numbers start with the biggest, names and pids with the smallest.
    pub fn default_desc(self) -> bool {
        matches!(self, SortKey::Cpu | SortKey::Mem | SortKey::Io)
    }
}

/// What the list shows and how.
#[derive(Debug, Clone)]
pub struct View {
    pub sort: SortKey,
    pub desc: bool,
    pub query: String,
    pub mine_only: bool,
    pub kernel: bool,
    pub my_uid: u32,
}

impl Default for View {
    fn default() -> View {
        View { sort: SortKey::Cpu, desc: true, query: String::new(), mine_only: false, kernel: false, my_uid: 0 }
    }
}

/// One line of the visible list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VRow {
    Section(Section, usize),
    Group(usize),
    /// `(group, index into the group's members)`
    Child(usize, usize),
}

fn cmp_f(a: f64, b: f64) -> std::cmp::Ordering {
    a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal)
}

fn order_by(view: &View, procs: &[Proc], a: (&str, usize, f64, u64, f64), b: (&str, usize, f64, u64, f64)) -> std::cmp::Ordering {
    let _ = procs;
    let o = match view.sort {
        SortKey::Name => a.0.to_lowercase().cmp(&b.0.to_lowercase()),
        SortKey::Pid => (a.1).cmp(&b.1),
        SortKey::Cpu => cmp_f(a.2, b.2),
        SortKey::Mem => a.3.cmp(&b.3),
        SortKey::Io => cmp_f(a.4, b.4),
    };
    // Ties fall back to the name and pid so the order is stable between refreshes.
    let o = if view.desc { o.reverse() } else { o };
    o.then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase())).then(a.1.cmp(&b.1))
}

fn matches(q: &str, p: &Proc) -> bool {
    p.name.to_lowercase().contains(q) || p.comm.to_lowercase().contains(q) || p.pid.to_string().contains(q) || p.cmdline.join(" ").to_lowercase().contains(q)
}

/// The visible rows: Apps then Background, each sorted, with groups in `expanded` (by main pid) unfolded.
/// A search unfolds the groups it matches inside and lists only the matching processes.
pub fn flatten(procs: &[Proc], groups: &[Group], view: &View, expanded: &HashSet<u32>) -> Vec<VRow> {
    flatten_pinned(procs, groups, view, expanded, None)
}

/// The pid that names a row (a group by its main process), what [`pin_order`] and [`flatten_pinned`] keep rows by.
pub fn row_pid(procs: &[Proc], groups: &[Group], row: &VRow) -> Option<u32> {
    match *row {
        VRow::Section(..) => None,
        VRow::Group(g) => Some(procs[groups[g].members[0]].pid),
        VRow::Child(g, m) => Some(procs[groups[g].members[m]].pid),
    }
}

/// Where each row of `rows` stands, by pid: hand it to [`flatten_pinned`] to keep that order across a refresh.
pub fn pin_order(procs: &[Proc], groups: &[Group], rows: &[VRow]) -> std::collections::HashMap<u32, usize> {
    rows.iter().enumerate().filter_map(|(i, r)| row_pid(procs, groups, r).map(|pid| (pid, i))).collect()
}

/// [`flatten`], but rows that were listed before (`pin`) keep their relative order instead of being re-sorted, and
/// new ones go last: the list stops moving under the pointer while its numbers keep updating.
pub fn flatten_pinned(procs: &[Proc], groups: &[Group], view: &View, expanded: &HashSet<u32>, pin: Option<&std::collections::HashMap<u32, usize>>) -> Vec<VRow> {
    let q = view.query.trim().to_lowercase();
    let searching = !q.is_empty();
    let visible = |i: usize| (view.kernel || !procs[i].kthread) && (!view.mine_only || procs[i].uid == view.my_uid);
    let mut rows = Vec::new();
    for section in [Section::Apps, Section::Background] {
        let mut list: Vec<(usize, Vec<usize>)> = Vec::new();
        for (gi, g) in groups.iter().enumerate().filter(|(_, g)| g.section == section) {
            let mut members: Vec<usize> = (0..g.members.len()).filter(|&mi| visible(g.members[mi])).collect();
            if members.is_empty() {
                continue;
            }
            if searching {
                let name_hit = g.name.to_lowercase().contains(&q);
                if !name_hit {
                    members.retain(|&mi| matches(&q, &procs[g.members[mi]]));
                }
                if members.is_empty() {
                    continue;
                }
            }
            list.push((gi, members));
        }
        let key = |gi: usize| {
            let g = &groups[gi];
            (g.name.as_str(), procs[g.members[0]].pid as usize, f64::from(g.cpu), g.rss, g.io)
        };
        list.sort_by(|a, b| order_by(view, procs, key(a.0), key(b.0)));
        if let Some(pin) = pin {
            let at = |gi: usize| pin.get(&procs[groups[gi].members[0]].pid).copied().unwrap_or(usize::MAX);
            list.sort_by_key(|(gi, _)| at(*gi)); // stable: new rows keep their sorted order among themselves
        }
        if list.is_empty() {
            continue;
        }
        rows.push(VRow::Section(section, list.len()));
        for (gi, mut members) in list {
            let g = &groups[gi];
            rows.push(VRow::Group(gi));
            let open = (searching || expanded.contains(&procs[g.members[0]].pid)) && g.members.len() > 1;
            if open {
                members.sort_by(|&a, &b| {
                    let k = |mi: usize| {
                        let p = &procs[g.members[mi]];
                        (p.name.as_str(), p.pid as usize, f64::from(p.cpu), p.rss, p.io_total())
                    };
                    order_by(view, procs, k(a), k(b))
                });
                if let Some(pin) = pin {
                    members.sort_by_key(|&mi| pin.get(&procs[g.members[mi]].pid).copied().unwrap_or(usize::MAX));
                }
                rows.extend(members.into_iter().map(|mi| VRow::Child(gi, mi)));
            }
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pid: u32, ppid: u32, comm: &str, cpu: f32, rss: u64) -> Proc {
        Proc { pid, ppid, start: pid as u64, comm: comm.into(), name: comm.into(), cmdline: vec![comm.into()], state: 'S', cpu, rss, ..Proc::default() }
    }

    fn app(name: &str, exec: &str, icon: &str) -> AppEntry {
        AppEntry { name: name.into(), exec: exec.into(), icon: icon.into(), ..AppEntry::default() }
    }

    fn index() -> AppIndex {
        AppIndex::new(vec![
            app("Firefox", "env MOZ=1 /usr/bin/firefox %u", "firefox"),
            app("Terminal", "kitty", "kitty"),
            app("Script", "sh -c foo", "x-script"),
            app("Editor", "python3 /opt/ed/editor.py", "editor"),
        ])
    }

    #[test]
    fn exec_keys() {
        assert_eq!(exec_key("env A=1 /usr/bin/firefox %u").as_deref(), Some("firefox"));
        assert_eq!(exec_key("sh -c 'x'"), None, "a shell is not an app");
        assert_eq!(exec_key("\"/opt/App Dir/app\" --x").as_deref(), Some("app"), "first word only");
        assert_eq!(exec_key("%U"), None);
    }

    #[test]
    fn finds_apps_by_comm_exec_and_interpreter_script() {
        let ix = index();
        assert_eq!(ix.find(&p(1, 0, "firefox", 0.0, 0)), Some(0));
        assert_eq!(ix.find(&p(2, 0, "FIREFOX", 0.0, 0)), Some(0), "case-insensitive");
        let mut py = p(3, 1, "python3", 0.0, 0);
        py.cmdline = vec!["/usr/bin/python3".into(), "-u".into(), "/opt/ed/editor.py".into()];
        assert_eq!(ix.find(&py), Some(3), "a script run by an interpreter");
        assert_eq!(ix.find(&p(4, 1, "bash", 0.0, 0)), None, "generic names never match");
        let mut k = p(5, 2, "firefox", 0.0, 0);
        k.kthread = true;
        assert_eq!(ix.find(&k), None);
    }

    fn sample() -> Vec<Proc> {
        vec![
            p(1, 0, "systemd", 0.0, 10),
            p(100, 1, "kitty", 5.0, 50),
            p(101, 100, "bash", 0.0, 5),
            p(102, 101, "sleep", 20.0, 1),
            p(200, 1, "firefox", 10.0, 500),
            p(201, 200, "firefox", 30.0, 300),
            p(202, 201, "Web Content", 0.0, 100),
            p(300, 1, "worker", 50.0, 20),
            p(301, 1, "other", 1.0, 40),
        ]
    }

    #[test]
    fn groups_apps_with_their_descendants_and_keeps_the_rest_flat() {
        let procs = sample();
        let g = build_groups(&procs, &index());
        let apps: Vec<_> = g.iter().filter(|g| g.section == Section::Apps).collect();
        assert_eq!(apps.len(), 2);
        let term = apps.iter().find(|g| g.name == "Terminal").unwrap();
        assert_eq!(term.members, vec![1, 2, 3], "kitty, its shell and the shell's child");
        assert_eq!(term.cpu, 25.0);
        let ff = apps.iter().find(|g| g.name == "Firefox").unwrap();
        assert_eq!(ff.members.len(), 3, "main, same-named child and the helper below it");
        assert_eq!(procs[ff.members[0]].pid, 200, "the topmost firefox is the main process");
        assert_eq!(ff.rss, 900);
        assert_eq!(g.iter().filter(|g| g.section == Section::Background).count(), 3, "systemd, worker, other");
    }

    #[test]
    fn a_pid_loop_or_missing_parent_does_not_hang() {
        let procs = vec![p(10, 11, "a", 0.0, 0), p(11, 10, "b", 0.0, 0), p(12, 999, "c", 0.0, 0)];
        assert_eq!(build_groups(&procs, &index()).len(), 3);
    }

    fn rows_pids(procs: &[Proc], groups: &[Group], rows: &[VRow]) -> Vec<String> {
        rows.iter()
            .map(|r| match *r {
                VRow::Section(s, n) => format!("{s:?}:{n}"),
                VRow::Group(g) => format!("G{}", procs[groups[g].members[0]].pid),
                VRow::Child(g, m) => format!("c{}", procs[groups[g].members[m]].pid),
            })
            .collect()
    }

    #[test]
    fn sorts_sections_and_children_and_folds() {
        let procs = sample();
        let groups = build_groups(&procs, &index());
        let mut view = View::default();
        let mut open = HashSet::new();
        let rows = flatten(&procs, &groups, &view, &open);
        // Apps by CPU desc: firefox (40) before kitty (25); background: worker 50, other 1, systemd 0.
        assert_eq!(rows_pids(&procs, &groups, &rows), ["Apps:2", "G200", "G100", "Background:3", "G300", "G301", "G1"]);
        open.insert(100);
        let rows = flatten(&procs, &groups, &view, &open);
        assert_eq!(rows_pids(&procs, &groups, &rows)[..6], ["Apps:2", "G200", "G100", "c102", "c100", "c101"], "children sorted by CPU too");
        view.sort = SortKey::Pid;
        view.desc = false;
        let rows = flatten(&procs, &groups, &view, &open);
        assert_eq!(rows_pids(&procs, &groups, &rows)[1..3], ["G100", "c100"]);
        view.sort = SortKey::Mem;
        view.desc = true;
        let rows = flatten(&procs, &groups, &view, &HashSet::new());
        assert_eq!(rows_pids(&procs, &groups, &rows)[1], "G200", "most memory first");
        view.sort = SortKey::Name;
        view.desc = false;
        let rows = flatten(&procs, &groups, &view, &HashSet::new());
        assert_eq!(rows_pids(&procs, &groups, &rows)[4..], ["G301", "G1", "G300"].map(String::from)[..], "other, systemd, worker");
    }

    #[test]
    fn search_filters_and_unfolds() {
        let procs = sample();
        let groups = build_groups(&procs, &index());
        let view = View { query: "sleep".into(), ..View::default() };
        let rows = flatten(&procs, &groups, &view, &HashSet::new());
        assert_eq!(rows_pids(&procs, &groups, &rows), ["Apps:1", "G100", "c102"], "the group and just the matching child");
        let view = View { query: "terminal".into(), ..View::default() };
        let rows = flatten(&procs, &groups, &view, &HashSet::new());
        assert_eq!(rows.iter().filter(|r| matches!(r, VRow::Child(..))).count(), 3, "a name match keeps every process");
        let view = View { query: "301".into(), ..View::default() };
        assert_eq!(rows_pids(&procs, &groups, &flatten(&procs, &groups, &view, &HashSet::new())), ["Background:1", "G301"], "pids match");
        let view = View { query: "nothing like this".into(), ..View::default() };
        assert!(flatten(&procs, &groups, &view, &HashSet::new()).is_empty());
    }

    #[test]
    fn kernel_threads_and_other_users_can_be_hidden() {
        let mut procs = sample();
        procs[8].kthread = true;
        procs[7].uid = 0;
        for q in &mut procs {
            if q.uid == 0 && q.pid != 300 {
                q.uid = 1000;
            }
        }
        let groups = build_groups(&procs, &index());
        let mut view = View { my_uid: 1000, ..View::default() };
        let all: Vec<_> = flatten(&procs, &groups, &view, &HashSet::new());
        assert!(!rows_pids(&procs, &groups, &all).contains(&"G301".to_string()), "kernel threads hidden by default");
        view.kernel = true;
        assert!(rows_pids(&procs, &groups, &flatten(&procs, &groups, &view, &HashSet::new())).contains(&"G301".to_string()));
        view.mine_only = true;
        assert!(!rows_pids(&procs, &groups, &flatten(&procs, &groups, &view, &HashSet::new())).contains(&"G300".to_string()), "root's process is not mine");
    }

    #[test]
    fn a_pinned_list_keeps_its_order_while_the_numbers_change() {
        let mut procs = sample();
        let view = View::default();
        let none = HashSet::new();
        let groups = build_groups(&procs, &index());
        let before = flatten(&procs, &groups, &view, &none);
        let pin = pin_order(&procs, &groups, &before);
        // kitty is suddenly the busiest app and worker has gone quiet: unpinned that re-sorts.
        procs[1].cpu = 90.0;
        procs[7].cpu = 0.0;
        let groups = build_groups(&procs, &index());
        let sorted = rows_pids(&procs, &groups, &flatten(&procs, &groups, &view, &none));
        assert_eq!(sorted[1], "G100", "the unpinned list re-sorts");
        let pinned = rows_pids(&procs, &groups, &flatten_pinned(&procs, &groups, &view, &none, Some(&pin)));
        assert_eq!(pinned, rows_pids(&procs, &groups, &before), "the pinned list stays put");
        // A process that appears later goes last in its section; one that exits just leaves.
        procs.push(p(400, 1, "newcomer", 99.0, 10));
        procs.retain(|x| x.pid != 301);
        let groups = build_groups(&procs, &index());
        let pinned = rows_pids(&procs, &groups, &flatten_pinned(&procs, &groups, &view, &none, Some(&pin)));
        assert_eq!(pinned, ["Apps:2", "G200", "G100", "Background:3", "G300", "G1", "G400"]);
    }
}
