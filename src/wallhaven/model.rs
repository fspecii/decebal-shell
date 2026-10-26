//! Results list and paging state machine. Stale replies (older searches) are dropped.

use super::api::{Item, Page};

#[derive(Debug, Default)]
pub struct Model {
    pub items: Vec<Item>,
    /// Last page fetched.
    pub page: u32,
    pub last_page: u32,
    /// Bumps on every new search so late replies can be recognised and ignored.
    pub generation: u64,
    pub loading: bool,
    pub error: String,
}

impl Model {
    /// Starts a fresh search and returns its generation.
    pub fn new_search(&mut self) -> u64 {
        self.generation += 1;
        self.items.clear();
        self.page = 0;
        self.last_page = 1;
        self.loading = false;
        self.error.clear();
        self.generation
    }

    /// The next page worth fetching, if any and nothing is in flight.
    pub fn next_page(&self) -> Option<u32> {
        if self.loading || self.page >= self.last_page {
            None
        } else {
            Some(self.page + 1)
        }
    }

    pub fn begin_fetch(&mut self) {
        self.loading = true;
    }

    /// Applies a network reply. Returns false when it belongs to an old search.
    pub fn apply_page(&mut self, generation: u64, page: u32, result: Result<Page, String>) -> bool {
        if generation != self.generation {
            return false;
        }
        self.loading = false;
        match result {
            Ok(p) => {
                if !p.items.is_empty() {
                    self.error.clear();
                }
                self.items.extend(p.items);
                self.page = page;
                self.last_page = p.last_page;
                if self.items.is_empty() {
                    self.error = "No wallpapers found".into();
                }
            }
            Err(_) if self.items.is_empty() => self.error = "Couldn't reach wallhaven.cc".into(),
            Err(_) => {}
        }
        true
    }

    /// Replaces the list with local wallpapers (Recent tab); no paging.
    pub fn set_local(&mut self, generation: u64, items: Vec<Item>) -> bool {
        if generation != self.generation {
            return false;
        }
        self.loading = false;
        self.page = 0;
        self.last_page = 0;
        self.error = if items.is_empty() { "No wallpapers used yet — pick one from Top or a search".into() } else { String::new() };
        self.items = items;
        true
    }

    pub fn has_more(&self) -> bool {
        self.page < self.last_page
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str) -> Item {
        Item { id: id.into(), thumb: String::new(), full: String::new(), resolution: "1x1".into(), favorites: Some(0) }
    }

    fn page(ids: &[&str], last: u32) -> Page {
        Page { items: ids.iter().map(|i| item(i)).collect(), last_page: last }
    }

    #[test]
    fn pages_accumulate_and_stop_at_last() {
        let mut m = Model::default();
        let g = m.new_search();
        assert_eq!(m.next_page(), Some(1));
        m.begin_fetch();
        assert_eq!(m.next_page(), None, "one request at a time");
        assert!(m.apply_page(g, 1, Ok(page(&["a", "b"], 2))));
        assert_eq!(m.next_page(), Some(2));
        m.begin_fetch();
        assert!(m.apply_page(g, 2, Ok(page(&["c"], 2))));
        assert_eq!(m.items.len(), 3);
        assert_eq!(m.next_page(), None);
        assert!(!m.has_more());
    }

    #[test]
    fn stale_reply_is_dropped() {
        let mut m = Model::default();
        let old = m.new_search();
        m.begin_fetch();
        let _new = m.new_search();
        assert!(!m.apply_page(old, 1, Ok(page(&["a"], 1))));
        assert!(m.items.is_empty());
        assert_eq!(m.next_page(), Some(1), "new search is free to fetch");
    }

    #[test]
    fn network_error_with_no_results_is_shown() {
        let mut m = Model::default();
        let g = m.new_search();
        m.begin_fetch();
        m.apply_page(g, 1, Err("timeout".into()));
        assert_eq!(m.error, "Couldn't reach wallhaven.cc");
        assert!(!m.loading);
    }

    #[test]
    fn network_error_after_results_keeps_them_quietly() {
        let mut m = Model::default();
        let g = m.new_search();
        m.apply_page(g, 1, Ok(page(&["a"], 5)));
        m.begin_fetch();
        m.apply_page(g, 2, Err("timeout".into()));
        assert_eq!(m.items.len(), 1);
        assert!(m.error.is_empty());
        assert_eq!(m.next_page(), Some(2), "retry possible");
    }

    #[test]
    fn empty_result_says_so() {
        let mut m = Model::default();
        let g = m.new_search();
        m.apply_page(g, 1, Ok(page(&[], 1)));
        assert_eq!(m.error, "No wallpapers found");
    }

    #[test]
    fn local_results_have_no_paging() {
        let mut m = Model::default();
        let g = m.new_search();
        assert!(m.set_local(g, vec![item("x")]));
        assert_eq!(m.next_page(), None);
        assert!(m.error.is_empty());
        let g2 = m.new_search();
        m.set_local(g2, vec![]);
        assert!(m.error.starts_with("No wallpapers used yet"));
    }
}
