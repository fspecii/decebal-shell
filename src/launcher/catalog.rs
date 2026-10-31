//! The application catalog the styled launchers share: installed apps grouped into freedesktop main categories,
//! pinned favourites, recently used apps and fuzzy search. Everything is real data; nothing is a placeholder list.

use crate::apps::AppEntry;
use crate::fuzzy::{self, History};
use std::path::PathBuf;

/// The freedesktop main categories (Desktop Menu Specification, appendix A), plus `Other` for the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Category {
    AudioVideo,
    Development,
    Education,
    Game,
    Graphics,
    Network,
    Office,
    Science,
    Settings,
    System,
    Utility,
    Other,
}

impl Category {
    pub const ALL: [Category; 12] = [
        Category::AudioVideo,
        Category::Development,
        Category::Education,
        Category::Game,
        Category::Graphics,
        Category::Network,
        Category::Office,
        Category::Science,
        Category::Settings,
        Category::System,
        Category::Utility,
        Category::Other,
    ];

    /// The label the desktop menus use for it.
    pub fn title(self) -> &'static str {
        match self {
            Category::AudioVideo => "Multimedia",
            Category::Development => "Development",
            Category::Education => "Education",
            Category::Game => "Games",
            Category::Graphics => "Graphics",
            Category::Network => "Internet",
            Category::Office => "Office",
            Category::Science => "Science",
            Category::Settings => "Settings",
            Category::System => "System",
            Category::Utility => "Utilities",
            Category::Other => "Other",
        }
    }

    /// The freedesktop icon-naming name of its menu icon (drawn from the user's icon theme).
    pub fn icon(self) -> &'static str {
        match self {
            Category::AudioVideo => "applications-multimedia",
            Category::Development => "applications-development",
            Category::Education => "applications-education",
            Category::Game => "applications-games",
            Category::Graphics => "applications-graphics",
            Category::Network => "applications-internet",
            Category::Office => "applications-office",
            Category::Science => "applications-science",
            Category::Settings => "preferences-desktop",
            Category::System => "applications-system",
            Category::Utility => "applications-utilities",
            Category::Other => "applications-other",
        }
    }

    /// The first main category in a `Categories=` value (`Audio`/`Video` count as AudioVideo).
    pub fn of(categories: &str) -> Category {
        categories
            .split(';')
            .find_map(|c| match c.trim() {
                "AudioVideo" | "Audio" | "Video" => Some(Category::AudioVideo),
                "Development" => Some(Category::Development),
                "Education" => Some(Category::Education),
                "Game" => Some(Category::Game),
                "Graphics" => Some(Category::Graphics),
                "Network" => Some(Category::Network),
                "Office" => Some(Category::Office),
                "Science" => Some(Category::Science),
                "Settings" => Some(Category::Settings),
                "System" => Some(Category::System),
                "Utility" => Some(Category::Utility),
                _ => None,
            })
            .unwrap_or(Category::Other)
    }
}

/// Where the launcher keeps its pinned apps: `launcher-pinned.json` next to `decebal.json`.
pub const PINNED_FILE: &str = "launcher-pinned.json";

/// The usage-history key of an app: its desktop-file id, stable even when two apps share a `Name=`.
fn history_key(app: &AppEntry) -> String {
    format!("appid:{}", app_id(app))
}

/// The older key Spotlight (and the launcher before) uses: the display name. It identifies an app only when no
/// other installed app has the same name.
fn legacy_key(app: &AppEntry) -> String {
    format!("app:{}", app.name)
}

/// The desktop-file id (`firefox` for `firefox.desktop`), else the name.
pub fn app_id(app: &AppEntry) -> String {
    std::path::Path::new(&app.file).file_stem().map(|s| s.to_string_lossy().into_owned()).filter(|s| !s.is_empty()).unwrap_or_else(|| app.name.clone())
}

fn exec_name(exec: &str) -> String {
    crate::launch::exec_argv(exec).first().and_then(|p| p.rsplit('/').next()).unwrap_or("").to_string()
}

pub struct Catalog {
    pub apps: Vec<AppEntry>,
    /// The category of each app.
    pub category: Vec<Category>,
    /// The categories that have at least one app, in menu order.
    pub present: Vec<Category>,
    /// Pinned apps in their order.
    pub favorites: Vec<usize>,
    /// Recently launched apps, most recent first.
    pub recent: Vec<usize>,
    history: History,
    /// Apps whose name no other app has (only those may use the name-keyed legacy history).
    unique_name: Vec<bool>,
    pinned_file: Option<PathBuf>,
}

impl Catalog {
    /// The installed apps with the user's pins and the shared usage history.
    pub fn load(apps: Vec<AppEntry>) -> Catalog {
        let path = crate::config::config_dir().join(PINNED_FILE);
        let pinned: Option<Vec<String>> = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| v.get("pinned").and_then(|p| p.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()));
        let mut c = Catalog::with(apps, pinned, History::load());
        c.pinned_file = Some(path);
        c
    }

    /// `pinned` = desktop ids saved earlier (`None` = never saved: start from the dock's pins, then common apps).
    pub fn with(apps: Vec<AppEntry>, pinned: Option<Vec<String>>, history: History) -> Catalog {
        let category: Vec<Category> = apps.iter().map(|a| Category::of(&a.categories)).collect();
        let present = Category::ALL.into_iter().filter(|c| category.contains(c)).collect();
        let mut names: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
        for a in &apps {
            *names.entry(a.name.as_str()).or_default() += 1;
        }
        let unique_name = apps.iter().map(|a| names[a.name.as_str()] == 1).collect();
        let mut c = Catalog { apps, category, present, favorites: Vec::new(), recent: Vec::new(), history, unique_name, pinned_file: None };
        c.favorites = match pinned {
            Some(ids) => ids.iter().filter_map(|id| c.find_id(id)).collect(),
            None => c.default_favorites(),
        };
        c.favorites.dedup();
        c.refresh_recent();
        c
    }

    fn find_id(&self, id: &str) -> Option<usize> {
        self.apps.iter().position(|a| app_id(a) == id)
    }

    /// The dock's pinned apps, else a browser, file manager, terminal, editor and settings, topped up alphabetically.
    fn default_favorites(&self) -> Vec<usize> {
        let mut out: Vec<usize> = if cfg!(test) { Vec::new() } else { crate::config::pinned().ordered().iter().filter_map(|id| self.find_id(id)).collect() };
        for want in ["WebBrowser", "FileManager", "TerminalEmulator", "TextEditor", "Settings", "Email", "Calculator"] {
            if let Some(i) = self.apps.iter().position(|a| a.categories.split(';').any(|c| c == want)) {
                if !out.contains(&i) {
                    out.push(i);
                }
            }
        }
        for i in 0..self.apps.len() {
            if out.len() >= 6 {
                break;
            }
            if !out.contains(&i) {
                out.push(i);
            }
        }
        out
    }

    /// The app a history key names: an `appid:` key by desktop id, a legacy `app:` name only when unambiguous.
    fn app_of_key(&self, key: &str) -> Option<usize> {
        if let Some(id) = key.strip_prefix("appid:") {
            return self.apps.iter().position(|a| app_id(a) == id);
        }
        let name = key.strip_prefix("app:")?;
        self.apps.iter().position(|a| a.name == name).filter(|&i| self.unique_name[i])
    }

    fn refresh_recent(&mut self) {
        let mut recent = Vec::new();
        for key in self.history.recent("", 500) {
            if let Some(i) = self.app_of_key(&key) {
                if !recent.contains(&i) {
                    recent.push(i);
                }
            }
            if recent.len() == 12 {
                break;
            }
        }
        self.recent = recent;
    }

    /// The ranking bonus of app `i`: its own history, or its name's when that name is unambiguous.
    fn boost(&self, i: usize, now: u64) -> i32 {
        let a = &self.apps[i];
        let legacy = if self.unique_name[i] { self.history.boost(&legacy_key(a), now) } else { 0 };
        self.history.boost(&history_key(a), now).max(legacy)
    }

    /// Every app, alphabetically (the list is sorted by name when scanned).
    pub fn all(&self) -> Vec<usize> {
        (0..self.apps.len()).collect()
    }

    pub fn in_category(&self, c: Category) -> Vec<usize> {
        (0..self.apps.len()).filter(|&i| self.category[i] == c).collect()
    }

    /// Fuzzy matches over the name, the program and the keywords; frequently used apps first among equals.
    pub fn search(&self, query: &str) -> Vec<usize> {
        let q = query.trim();
        if q.is_empty() {
            return self.all();
        }
        let now = fuzzy::now();
        let mut scored: Vec<(i32, usize)> = self
            .apps
            .iter()
            .enumerate()
            .filter_map(|(i, a)| {
                let exec = exec_name(&a.exec);
                let mut fields: Vec<&str> = vec![&a.name, &exec];
                fields.extend(a.keywords.split(';').map(str::trim).filter(|k| !k.is_empty()).take(12));
                let s = fuzzy::best(q, &fields)?;
                Some((s + self.boost(i, now), i))
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| self.apps[a.1].name.len().cmp(&self.apps[b.1].name.len())).then(a.1.cmp(&b.1)));
        scored.into_iter().map(|(_, i)| i).collect()
    }

    pub fn is_favorite(&self, i: usize) -> bool {
        self.favorites.contains(&i)
    }

    /// Pins or unpins app `i` and saves the list; true when it is pinned now.
    pub fn toggle_favorite(&mut self, i: usize) -> bool {
        if i >= self.apps.len() {
            return false;
        }
        let pinned = if let Some(p) = self.favorites.iter().position(|&f| f == i) {
            self.favorites.remove(p);
            false
        } else {
            self.favorites.push(i);
            true
        };
        if let Some(path) = &self.pinned_file {
            let ids: Vec<String> = self.favorites.iter().map(|&f| app_id(&self.apps[f])).collect();
            if let Err(e) = crate::config::write_setting(path, "pinned", serde_json::json!(ids)) {
                crate::log_error!("launcher pins not saved: {e}");
            }
        }
        pinned
    }

    /// Starts app `i` (the same typed path as the other launchers) and remembers the use.
    pub fn launch(&mut self, i: usize) -> bool {
        let Some(app) = self.apps.get(i) else { return false };
        if !crate::launch::spawn_app(app) {
            return false;
        }
        // The desktop id always; the name too while it is unambiguous, so Spotlight's ranking keeps learning.
        let mut keys = vec![history_key(app)];
        if self.unique_name[i] {
            keys.push(legacy_key(app));
        }
        let now = fuzzy::now();
        // A real launcher merges into the file other programs write too; tests keep it in memory.
        let mut history = if self.pinned_file.is_some() { History::load() } else { std::mem::take(&mut self.history) };
        for k in &keys {
            history.bump(k, now);
        }
        if self.pinned_file.is_some() {
            history.save();
        }
        self.history = history;
        self.refresh_recent();
        true
    }

    /// The person signed in, for the styles that greet them (Windows, Kickoff, Whisker).
    pub fn user_name() -> String {
        crate::lock::current_user().map(|u| u.display).filter(|s| !s.is_empty()).or_else(|| std::env::var("USER").ok()).unwrap_or_else(|| "User".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn app(name: &str, categories: &str) -> AppEntry {
        AppEntry { name: name.into(), exec: format!("{} %U", name.to_lowercase()), categories: categories.into(), file: format!("/x/{}.desktop", name.to_lowercase()), ..Default::default() }
    }

    fn sample() -> Vec<AppEntry> {
        vec![
            app("Calculator", "Utility;Calculator;"),
            app("Files", "System;FileManager;"),
            app("Firefox", "Network;WebBrowser;"),
            AppEntry { keywords: "photo;paint;".into(), ..app("Gimp", "Graphics;2DGraphics;") },
            app("Mystery", ""),
            app("Terminal", "System;TerminalEmulator;"),
            app("Videos", "Video;Player;"),
        ]
    }

    #[test]
    fn main_categories_follow_the_spec_and_unknown_is_other() {
        assert_eq!(Category::of("GTK;Video;Player;"), Category::AudioVideo);
        assert_eq!(Category::of("Qt;KDE;Development;IDE;"), Category::Development);
        assert_eq!(Category::of("X-Custom;"), Category::Other);
        let c = Catalog::with(sample(), Some(vec![]), History::default());
        assert_eq!(c.present, [Category::AudioVideo, Category::Graphics, Category::Network, Category::System, Category::Utility, Category::Other]);
        assert_eq!(c.in_category(Category::System), [1, 5]);
    }

    #[test]
    fn search_uses_names_programs_and_keywords() {
        let c = Catalog::with(sample(), Some(vec![]), History::default());
        assert_eq!(c.search("fire").first(), Some(&2));
        assert_eq!(c.search("paint"), [3], "keywords match");
        assert!(c.search("zzzz").is_empty());
        assert_eq!(c.search("").len(), 7);
    }

    #[test]
    fn history_ranks_and_lists_recent_apps() {
        let mut h = History::default();
        h.bump("app:Files", 100);
        for _ in 0..3 {
            h.bump("app:Firefox", fuzzy::now());
        }
        let c = Catalog::with(sample(), Some(vec![]), h);
        assert_eq!(c.recent, [2, 1]);
        assert_eq!(c.search("fi")[0], 2, "the frequently used of two close matches first");
    }

    #[test]
    fn duplicate_names_never_pick_the_wrong_app_from_history() {
        let mut apps = sample();
        // two different terminals both called "Terminal"
        apps.push(AppEntry { exec: "other-term".into(), file: "/y/other-term.desktop".into(), ..app("Terminal", "System;TerminalEmulator;") });
        let other = apps.len() - 1;
        let now = fuzzy::now();
        let mut h = History::default();
        h.bump("app:Terminal", now); // legacy, ambiguous: must not resolve to either
        h.bump("app:Files", now - 10); // legacy, unique: still honoured
        h.bump("appid:other-term", now - 20); // new, by desktop id
        let c = Catalog::with(apps, Some(vec![]), h);
        assert_eq!(c.recent, [1, other], "the ambiguous name is skipped, the id finds the second Terminal");
        assert_eq!(c.boost(5, now), 0, "the first Terminal gets no credit for an ambiguous name");
        assert!(c.boost(other, now) > 0 && c.boost(1, now) > 0);
    }

    #[test]
    fn launching_records_the_desktop_id_and_a_unique_name_only() {
        let mut apps = sample();
        apps.push(AppEntry { exec: "true".into(), file: "/y/term2.desktop".into(), ..app("Terminal", "System;") });
        apps[0].exec = "true".into();
        let dup = apps.len() - 1;
        let mut c = Catalog::with(apps, Some(vec![]), History::default());
        assert!(c.launch(dup));
        assert!(c.launch(0));
        let keys = c.history.recent("", 10);
        assert!(keys.contains(&"appid:term2".to_string()) && keys.contains(&"appid:calculator".to_string()));
        assert!(keys.contains(&"app:Calculator".to_string()), "a unique name keeps Spotlight's key current");
        assert!(!keys.contains(&"app:Terminal".to_string()), "an ambiguous name is never written");
        // both launched within the same second: the order between them is not defined, the set is
        let mut recent = c.recent.clone();
        recent.sort();
        assert_eq!(recent, [0, dup]);
    }

    #[test]
    fn pins_load_by_id_default_to_common_apps_and_toggle() {
        let c = Catalog::with(sample(), Some(vec!["terminal".into(), "gone".into(), "gimp".into()]), History::default());
        assert_eq!(c.favorites, [5, 3], "unknown ids are skipped, order kept");
        let mut c = Catalog::with(sample(), None, History::default());
        assert_eq!(&c.favorites[..4], [2, 1, 5, 0], "browser, files, terminal, then calculator");
        assert_eq!(c.favorites.len(), 6, "topped up to a full row");
        assert!(!c.toggle_favorite(2));
        assert!(!c.is_favorite(2));
        assert!(c.toggle_favorite(2));
        assert_eq!(c.favorites.last(), Some(&2));
        assert!(!c.toggle_favorite(99));
    }
}
