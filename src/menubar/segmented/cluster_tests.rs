//! The generic Dux cluster layout (`bar.cluster`): numbers checked against the pinned UKUI panel source, no theme id anywhere.
//!
//! Source (openkylin/ukui-panel @ 3429618, `panel/qml/org.ukui.panelNext/ui/MainContainer.qml`, `ApplicationsIsland.qml`, `SettingsIsland.qml`, `panel/src/view/panel-next.cpp`):
//! * the three islands sit in one `GridLayout` whose `columnSpacing` is **8**; the container's width is `data + settings + app + 8 * (islands - 1)` unless some island
//!   `fillWidth` (every plugin sets `Layout.fillWidth: !isHorizontal`, so never on a horizontal panel) and it is centred (`anchors.horizontalCenter`): content-sized, centred;
//! * an island is `panel height 56 - 2 * 4` = **48** high with radius `kRadiusWindow` = **12** and a 1 px border; the application island pads its content **12** each side and
//!   spaces items **4** apart; the window keeps **8** below the 56 px strip, so a face stands 4 + 8 = **12** above the screen edge.
use super::*;

const W: f32 = 1920.0;
const ITEM_GAP: f32 = 4.0;
const GROUP_GAP: f32 = 8.0;
const PAD: f32 = 12.0;
const FACE_H: f32 = 48.0;
const RADIUS: f32 = 12.0;
const CELL: f32 = 28.0;

fn cells(n: usize) -> Vec<(Kind, f32)> {
    (0..n).map(|i| (Kind::Tray(Tray::Task(i as u64 + 1)), CELL)).collect()
}

fn run(width: f32, groups: [Vec<(Kind, f32)>; 3]) -> Layout {
    // margin 7 and bar height 58 are the package's numbers: face top = margin + 5, face height = bar height - 10
    place(width, 11.0, ITEM_GAP, FACE_H, 12.0, RADIUS, groups, Some((GROUP_GAP, PAD)))
}

fn content(n: usize) -> f32 {
    n as f32 * CELL + (n.saturating_sub(1)) as f32 * ITEM_GAP + 2.0 * PAD
}

#[test]
fn three_islands_are_one_content_sized_run_centred_on_the_bar() {
    let lay = run(W, [cells(1), cells(4), cells(6)]);
    assert!(lay.cluster && lay.hidden.is_empty());
    assert_eq!(lay.groups.len(), 3);
    let widths = [content(1), content(4), content(6)];
    for (g, w) in lay.groups.iter().zip(widths) {
        assert!((g.w - w).abs() < 1e-3, "face width {} != content {w}", g.w);
        assert_eq!((g.h, g.y, g.radius), (FACE_H, 12.0, RADIUS));
    }
    // faces follow each other by exactly the 8 px column spacing
    for pair in lay.groups.windows(2) {
        assert!((pair[1].x - (pair[0].x + pair[0].w) - GROUP_GAP).abs() < 1e-3);
    }
    // the run is data + app + settings + 2 * 8 wide and its middle is the bar's middle
    let run_w = widths.iter().sum::<f32>() + 2.0 * GROUP_GAP;
    let (first, last) = (lay.groups.first().unwrap(), lay.groups.last().unwrap());
    assert!((last.x + last.w - first.x - run_w).abs() < 1e-3);
    assert!(((first.x + last.x + last.w) / 2.0 - W / 2.0).abs() < 1e-3);
    // items keep the 4 px item gap and the 12 px face padding
    let app: Vec<_> = lay.items.iter().filter(|i| i.x >= lay.groups[1].x && i.x < lay.groups[1].x + lay.groups[1].w).collect();
    assert_eq!(app.len(), 4);
    assert!((app[0].x - (lay.groups[1].x + PAD)).abs() < 1e-3);
    assert!((app[1].x - (app[0].x + CELL + ITEM_GAP)).abs() < 1e-3);
}

#[test]
fn the_run_is_sized_by_its_content_not_by_the_screen() {
    let a = run(W, [cells(1), cells(2), cells(3)]);
    let b = run(W, [cells(1), cells(3), cells(3)]);
    let width = |l: &Layout| l.groups.last().map(|g| g.x + g.w).unwrap() - l.groups[0].x;
    assert!((width(&b) - width(&a) - (CELL + ITEM_GAP)).abs() < 1e-3, "one more cell widens the run by one cell and one item gap");
    // and it re-centres: the left edge moves left by half of that
    assert!((a.groups[0].x - b.groups[0].x - (CELL + ITEM_GAP) / 2.0).abs() < 1e-3);
    // a wider or narrower bar only moves the run, never resizes it
    let wide = run(3840.0, [cells(1), cells(2), cells(3)]);
    assert!((width(&wide) - width(&a)).abs() < 1e-3);
    assert!((wide.groups[0].x - a.groups[0].x - (3840.0 - W) / 2.0).abs() < 1e-3);
}

#[test]
fn an_empty_group_has_no_face_and_no_gap() {
    let lay = run(W, [cells(1), vec![], cells(2)]);
    assert_eq!(lay.groups.len(), 2);
    assert!((lay.groups[1].x - (lay.groups[0].x + lay.groups[0].w) - GROUP_GAP).abs() < 1e-3);
    let run_w = content(1) + content(2) + GROUP_GAP;
    assert!(((lay.groups[0].x + lay.groups[1].x + lay.groups[1].w) / 2.0 - W / 2.0).abs() < 1e-3);
    assert!((lay.groups[1].x + lay.groups[1].w - lay.groups[0].x - run_w).abs() < 1e-3);
}

#[test]
fn a_short_output_gives_up_detail_then_whole_groups_and_never_overlaps() {
    for width in [1920.0, 1280.0, 900.0, 640.0, 480.0, 360.0, 320.0, 240.0, 160.0] {
        let lay = run(width, [cells(1), cells(8), cells(12)]);
        let mut right = f32::MIN;
        for g in &lay.groups {
            assert!(g.x >= 11.0 - 1e-3 && g.x + g.w <= width - 11.0 + 1e-3, "{width}: face {}..{} leaves the bar", g.x, g.x + g.w);
            assert!(g.x >= right - 1e-3, "{width}: faces overlap");
            right = g.x + g.w + GROUP_GAP - 1e-3;
        }
        for i in &lay.items {
            assert!(lay.groups.iter().any(|g| i.x >= g.x - 1e-3 && i.x + i.w <= g.x + g.w + 1e-3), "{width}: an item is painted outside every face");
        }
    }
    let crowded = run(480.0, [cells(1), cells(8), cells(12)]);
    assert!(!crowded.hidden.is_empty(), "what did not fit stays reachable through the overflow list");
}

#[test]
fn the_stock_placement_is_unchanged_without_a_cluster() {
    let stock = place(W, 11.0, ITEM_GAP, FACE_H, 12.0, RADIUS, [cells(1), cells(4), cells(6)], None);
    assert!(!stock.cluster);
    let (l, r) = (&stock.groups[0], &stock.groups[2]);
    assert_eq!(l.x, 11.0, "the first group still starts at the margin");
    assert!((r.x + r.w - (W - 11.0)).abs() < 1e-3, "the last group still ends at the margin");
    assert_eq!(stock.faces().len(), 11, "faces keep their per-item categories (one per task cell here), not one per group");
}

#[test]
fn bar_layout_reads_the_cluster_keys_clamped_and_accepts_registry_keys() {
    let l = crate::config::BarLayout::from_json(&serde_json::json!({"cluster": true, "group_gap": 99, "group_padding": -4, "left": ["launcher"], "center": ["tasks", "search"], "right": ["systemTray", "bogus"]}));
    assert!(l.cluster);
    assert_eq!((l.group_gap, l.group_padding), (48.0, 0.0));
    assert_eq!((l.left.len(), l.center.len(), l.right.len()), (1, 2, 1), "registry keys stay, unknown ids go");
    let d = crate::config::BarLayout::default();
    assert!(!d.cluster, "no cluster unless asked");
}

#[test]
fn a_manifest_may_ask_for_a_cluster_and_is_checked() {
    let head = "format = 1\nid = \"c\"\nname = \"C\"\nversion = \"1.0.0\"\nauthor = \"t\"\nlicense = \"MIT\"\nmin_shell_version = \"0.3.0\"\n[colors]\nbackground = \"#101010\"\nsurface = \"#202020\"\ntext = \"#f0f0f0\"\naccent = \"#3676f5\"\naccent_text = \"#ffffff\"\n";
    let ok = |bar: &str| {
        let a = crate::theme::engine::manifest::analyze(&format!("{head}[bar]\nstyle = \"dux\"\n{bar}"));
        a.diags.into_iter().filter(|d| d.level == crate::theme::engine::manifest::Level::Error && d.key.starts_with("bar")).map(|d| d.to_string()).collect::<Vec<_>>()
    };
    assert_eq!(ok("cluster = true\ngroup_gap = 8\ngroup_padding = 12\ncenter = [\"tasks\", \"search\"]\n"), Vec::<String>::new());
    assert!(ok("group_gap = 99\n").iter().any(|e| e.contains("group_gap")), "out of range");
    assert!(ok("center = [\"bogus\"]\n").iter().any(|e| e.contains("bogus")), "unknown ids are still refused");
}
