//! What a bar draws from an exported menu: top-level titles and the rows of one submenu, with
//! hidden items removed and separators normalised. Every row keeps the exporter's item id, which
//! is what a click must send back ([`super::AppMenus::activate`]).

use super::tree::{MenuItem, MenuTree, ToggleState, ToggleType};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Title {
    pub id: i32,
    pub label: String,
    pub mnemonic: Option<char>,
    pub enabled: bool,
    /// A top-level entry without children is itself a command (rare, but exporters do it).
    pub submenu: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub id: i32,
    pub label: String,
    pub shortcut: String,
    pub enabled: bool,
    pub separator: bool,
    pub toggle: ToggleType,
    pub state: ToggleState,
    pub submenu: bool,
    pub icon_name: String,
}

impl Row {
    /// Whether clicking the row should send `clicked` (not a separator, submenu or disabled row).
    pub fn activatable(&self) -> bool {
        self.enabled && !self.separator && !self.submenu
    }

    /// The bar's text marker for a toggle: `✓` for a checked item, `•` for a selected radio,
    /// `–` for an indeterminate one.
    pub fn mark(&self) -> &'static str {
        match (self.toggle, self.state) {
            (ToggleType::None, _) | (_, ToggleState::Off) => "",
            (ToggleType::Check, ToggleState::On) => "✓",
            (ToggleType::Radio, ToggleState::On) => "•",
            (_, ToggleState::Mixed) => "–",
        }
    }
}

/// Visible, labelled top-level entries (the bar's titles after the shell's own menus).
pub fn titles(tree: &MenuTree) -> Vec<Title> {
    tree.root
        .children
        .iter()
        .filter(|i| i.visible && !i.is_separator())
        .filter_map(|i| {
            let (label, mnemonic) = super::tree::split_mnemonic(&i.label);
            let label = label.trim().to_string();
            (!label.is_empty()).then_some(Title { id: i.id, label, mnemonic, enabled: i.enabled, submenu: i.is_submenu() })
        })
        .collect()
}

/// Visible rows under `parent` (`None` when `parent` is unknown). Leading, trailing and repeated
/// separators are dropped so hidden items never leave gaps.
pub fn rows(tree: &MenuTree, parent: i32) -> Option<Vec<Row>> {
    let parent = if parent == tree.root.id { &tree.root } else { tree.find(parent)? };
    let mut out: Vec<Row> = Vec::new();
    for i in parent.children.iter().filter(|i| i.visible) {
        if i.is_separator() {
            if out.last().is_some_and(|r| !r.separator) {
                out.push(row(i));
            }
            continue;
        }
        out.push(row(i));
    }
    while out.last().is_some_and(|r| r.separator) {
        out.pop();
    }
    Some(out)
}

fn row(i: &MenuItem) -> Row {
    Row {
        id: i.id,
        label: i.display_label(),
        shortcut: i.shortcut_text(),
        enabled: i.enabled,
        separator: i.is_separator(),
        toggle: i.toggle_type,
        state: i.toggle_state,
        submenu: i.is_submenu(),
        icon_name: i.icon_name.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::appmenu::tree::ItemKind;

    fn item(id: i32, label: &str) -> MenuItem {
        let mut i = MenuItem::new(id);
        i.label = label.into();
        i
    }
    fn sep(id: i32) -> MenuItem {
        let mut i = MenuItem::new(id);
        i.kind = ItemKind::Separator;
        i
    }

    fn tree() -> MenuTree {
        let mut hidden = item(12, "Hidden");
        hidden.visible = false;
        let mut check = item(14, "_Wrap");
        check.toggle_type = ToggleType::Check;
        check.toggle_state = ToggleState::On;
        let mut file = item(1, "_File");
        file.children = vec![sep(9), item(10, "_New"), sep(11), hidden, sep(13), check, sep(15)];
        let mut gone = item(2, "Gone");
        gone.visible = false;
        let mut root = MenuItem::new(0);
        root.children = vec![file, gone, sep(3), item(4, "  "), item(5, "_About")];
        MenuTree::from_items(1, root)
    }

    #[test]
    fn titles_skip_hidden_separators_and_blank_labels() {
        let t = titles(&tree());
        assert_eq!(t.iter().map(|t| (t.id, t.label.as_str(), t.submenu)).collect::<Vec<_>>(), [(1, "File", true), (5, "About", false)]);
        assert_eq!(t[0].mnemonic, Some('f'));
    }

    #[test]
    fn rows_collapse_separators_around_hidden_items() {
        let r = rows(&tree(), 1).unwrap();
        assert_eq!(r.iter().map(|r| r.id).collect::<Vec<_>>(), [10, 11, 14]);
        assert_eq!(r[0].label, "New");
        assert_eq!(r[2].mark(), "✓");
        assert!(r[0].activatable() && !r[1].activatable());
        assert!(rows(&tree(), 999).is_none());
        assert_eq!(rows(&tree(), 0).unwrap().iter().map(|r| r.id).collect::<Vec<_>>(), [1, 3, 4, 5]);
    }
}
