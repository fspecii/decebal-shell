use super::model::tests_support::model_with;
use super::model::CtlVal;
use super::packages::{apply, items};
use super::ui::Item;
use crate::theme::live::Source;

#[test]
fn the_section_lists_a_card_per_theme_and_the_stub_rows() {
    let (m, _) = model_with("[general]\n");
    let its = items(&m);
    let Some(Item::ThemeCards(cards)) = its.iter().find(|i| matches!(i, Item::ThemeCards(_))) else { panic!("no cards") };
    assert!(cards.len() >= crate::theme::builtin::BUILTIN.iter().filter(|(id, _)| !crate::theme::palette::legacy_theme(id)).count());
    assert_eq!(cards.iter().filter(|c| c.current).count(), 1);
    let nord = cards.iter().find(|c| c.id == "macos").unwrap();
    assert!(nord.meta.contains("1.0.0") && nord.meta.contains("built-in") && !nord.deletable, "{}", nord.meta);
    let ctls: Vec<(String, bool)> =
        its.iter().filter_map(|i| if let Item::Card(rows) = i { Some(rows) } else { None }).flat_map(|r| r.iter().flat_map(|r| r.ctls.iter().map(|c| (c.id.clone(), c.enabled)))).collect();
    for want in ["th:snapname", "th:install"] {
        assert!(ctls.iter().any(|(i, _)| i == want), "{want} in {ctls:?}");
    }
    assert!(ctls.contains(&("th:market".to_string(), false)), "the market button is a disabled stub");
}

#[test]
fn apply_on_a_card_runs_the_whole_engine_transaction() {
    let (mut m, cfg) = model_with("# mine\n[general]\n");
    m.apply("l:tcard:apply:windows11", CtlVal::Click);
    assert_eq!(Source { shell_json: m.paths.shell_json(), themes_dir: Source::discover().themes_dir }.selection().name, "windows11");
    assert_eq!(m.dock["style"], "windows", "the theme's dock style came with it");
    assert_eq!(m.shell["bar"]["enabled"], "auto");
    assert!(std::fs::read_to_string(&cfg).unwrap().starts_with("# mine\n"));
    assert!(m.notice.as_ref().is_some_and(|n| !n.error && n.text.contains("Applied")), "{:?}", m.notice);
    // delete needs a second click, and a built-in cannot be removed at all
    m.apply("l:tcard:delete:nord", CtlVal::Click);
    assert!(m.notice.as_ref().unwrap().text.contains("again"));
    m.apply("l:tcard:delete:nord", CtlVal::Click);
    assert!(m.notice.as_ref().unwrap().error, "built-in themes are not removable");
    // ids that are not the cards' are left to the rest of the pane
    assert!(!apply(&mut m, &["l", "tcard", "bogus", "x"], &CtlVal::Click));
    assert!(!apply(&mut m, &["th", "mode"], &CtlVal::Click));
}
