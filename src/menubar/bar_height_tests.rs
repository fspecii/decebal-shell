//! The declared bar height (`[bar] height`) of a classic or floating bar, one number for everything that depends on it: the surface and its
//! strut, the drawn strip, the applet positions and hit targets, popup placement, `bar_bottom` (notifications, dashboards) and the preview.
//! Checked on all four edges, at display scales of 100, 125 and 200 %, with scaled text and on a short output. A theme that declares nothing
//! keeps the classic 26 px strip and the 40 px rail.
use super::applets::tests::{fixture, items, reset};
use super::*;
use crate::app::App;
use crate::panel::{AppletSet, Edge, Renderer};

const SCALES: [f32; 3] = [1.0, 1.25, 2.0];

#[test]
fn fractional_declared_height_reserves_the_same_bottom_inset_as_its_surface() {
    let _t = themed(Some(32.5), 1.0);
    let bar = fixture(Renderer::MenuBar, Edge::Bottom, AppletSet::all(), 1280);
    assert_eq!(bar.surface_spec().unwrap().reserve, 33);
    assert_eq!(crate::theme::th().bar_insets().bottom, 33);
    reset();
}

fn themed(height: Option<f32>, text: f32) -> crate::theme::live::ThreadTheme {
    let mut t = crate::theme::builtin::def("nord").unwrap().resolve("nord", true, &crate::theme::model::Selection::default());
    t.bar_alpha = 1.0;
    t.font_scale = text;
    t.bar_height_declared = height.is_some();
    if let Some(h) = height {
        t.bar_height = h;
    }
    crate::theme::live::thread_theme(t)
}

/// What a horizontal bar of `style` on `edge` reserves and draws at `scale`: (reserve, pixels, strip top, strip height), all logical but the pixels.
fn drawn(style: Renderer, edge: Edge, scale: f32) -> (i32, Pixmap, f32, f32) {
    let bar = fixture(style, edge, AppletSet::all(), 1280);
    let (lw, lh) = (bar.width, bar.height);
    let reserve = bar.surface_spec().unwrap().reserve;
    let (top, h) = (bar.strip_top(), bar.bar_bottom());
    let mut sc = crate::backend::scaled::Scaled::new(bar, scale);
    let (pw, ph) = ((lw as f32 * scale).round() as u32, (lh as f32 * scale).round() as u32);
    sc.resized(pw, ph);
    let mut pm = Pixmap::new(pw, ph).unwrap();
    sc.draw(&mut pm);
    (reserve, pm, top, h)
}

fn alpha(pm: &Pixmap, x: u32, y: u32) -> u8 {
    pm.pixel(x, y).map_or(0, |p| p.alpha())
}

/// The vertical extent of everything drawn on the strip that differs from the strip's own fill, in physical rows from the strip's top.
fn ink_rows(pm: &Pixmap, first: u32, rows: u32) -> (u32, u32) {
    let bg = pm.pixel(1, first + rows / 2).unwrap();
    let differs = |x: u32, y: u32| {
        let p = pm.pixel(x, y).unwrap();
        (p.red() as i32 - bg.red() as i32).abs().max((p.green() as i32 - bg.green() as i32).abs()).max((p.blue() as i32 - bg.blue() as i32).abs()) > 14
    };
    let (mut lo, mut hi) = (u32::MAX, 0);
    for y in first..first + rows {
        if (0..pm.width()).any(|x| differs(x, y)) {
            lo = lo.min(y - first);
            hi = hi.max(y - first);
        }
    }
    (lo, hi)
}

#[test]
fn the_declared_height_is_one_generic_number_and_an_undeclared_bar_keeps_its_defaults() {
    use crate::panel::Edge as E;
    {
        let _t = themed(None, 1.0);
        let th = crate::theme::th();
        assert!(!th.bar_height_declared);
        assert_eq!((th.flat_bar_height(), th.rail_thickness()), (26.0, 40.0));
        for edge in E::ALL {
            assert_eq!(th.bar_thickness(edge), if edge.is_vertical() { 40.0 } else { 26.0 }, "{edge:?}");
        }
        assert_eq!(crate::theme::bar_bottom_for(&th, true), 26.0);
        // text scale does not change an undeclared bar
        drop(th);
        let _t2 = themed(None, 1.4);
        assert_eq!(crate::theme::th().flat_bar_height(), 26.0);
    }
    {
        let _t = themed(Some(32.0), 1.0);
        let th = crate::theme::th();
        assert_eq!((th.flat_bar_height(), th.rail_thickness()), (32.0, 40.0), "a rail keeps room for its stacked cells");
        for edge in E::ALL {
            assert_eq!(th.bar_thickness(edge), if edge.is_vertical() { 40.0 } else { 32.0 }, "{edge:?}");
        }
        assert_eq!(crate::theme::bar_bottom_for(&th, true), 32.0, "notifications and popups clear the declared bar");
        assert_eq!(crate::theme::bar_bottom_for(&th, false), 0.0);
    }
    {
        // a declared height never makes a rail thinner than it needs, and can make it thicker
        let _t = themed(Some(52.0), 1.0);
        let th = crate::theme::th();
        assert_eq!((th.flat_bar_height(), th.rail_thickness()), (52.0, 52.0));
    }
    {
        // scaled text raises the floor under a declared height: 12 px * 1.4 + 10 = 27; at scale 1 the floor is 22
        let _t = themed(Some(20.0), 1.0);
        assert_eq!(crate::theme::th().flat_bar_height(), 22.0);
        drop(_t);
        let _t = themed(Some(20.0), 1.4);
        assert_eq!(crate::theme::th().flat_bar_height(), 27.0);
        drop(_t);
        let _t = themed(Some(40.0), 1.4);
        assert_eq!(crate::theme::th().flat_bar_height(), 40.0, "a taller declaration is kept");
    }
}

#[test]
fn a_declared_height_is_the_surface_the_strut_the_drawn_strip_and_the_hit_targets_on_top_and_bottom_at_every_scale() {
    for style in [Renderer::MenuBar, Renderer::Floating] {
        for edge in [Edge::Top, Edge::Bottom] {
            for scale in SCALES {
                let _t = themed(Some(32.0), 1.0);
                let (reserve, pm, top, h) = drawn(style, edge, scale);
                let what = format!("{} {} {scale}", style.key(), edge.as_str());
                assert_eq!((reserve, h), (32, 32.0), "{what}: the strut and the strip are the declared height");
                let (first, rows) = ((top * scale).round() as u32, (32.0 * scale).round() as u32);
                if edge == Edge::Bottom {
                    assert_eq!(first + rows, pm.height(), "{what}: the strip ends at the surface's last row");
                } else {
                    assert_eq!(first, 0, "{what}");
                }
                if style == Renderer::MenuBar {
                    // the flat strip is filled for its whole declared height and not a row beyond it
                    for y in [first, first + rows / 2, first + rows - 1] {
                        assert_eq!(alpha(&pm, 1, y), 255, "{what}: row {y} of the strip is filled");
                    }
                    let outside = if edge == Edge::Top { first + rows + 2 } else { first.saturating_sub(2) };
                    assert_eq!(alpha(&pm, 1, outside), 0, "{what}: nothing is painted beyond the strip");
                }
                // the content is centred in the strip and not clipped by it
                let (lo, hi) = ink_rows(&pm, first, rows);
                assert!(lo >= 1 && hi + 2 <= rows, "{what}: content rows {lo}..{hi} inside the {rows}-row strip");
                let centre = (lo + hi) as f32 / 2.0;
                assert!((centre - rows as f32 / 2.0).abs() <= 3.0 * scale, "{what}: content centred ({centre} vs {})", rows as f32 / 2.0);
            }
        }
    }
}

#[test]
fn hit_targets_popups_and_input_regions_follow_the_declared_strip() {
    for edge in [Edge::Top, Edge::Bottom] {
        for scale in SCALES {
            let _t = themed(Some(32.0), 1.0);
            let bar = fixture(Renderer::MenuBar, edge, AppletSet::all(), 1280);
            let rects = items(&bar);
            let top = bar.strip_top();
            assert!(!rects.is_empty());
            for (kind, r) in &rects {
                assert_eq!((r.1, r.3), (top, 32.0), "{edge:?} {kind:?}: the target is the whole declared strip");
            }
            // the strip is the glass region too
            assert!(bar.glass_regions().iter().any(|g| (g.y - top).abs() < 0.01 && (g.h - 32.0).abs() < 0.01), "{edge:?}");
            // a press at the middle of the sound applet, through the display scale, opens its popup clear of the declared strip
            let (lw, lh) = (bar.width, bar.height);
            let mut sc = crate::backend::scaled::Scaled::new(bar, scale);
            sc.resized((lw as f32 * scale).round() as u32, (lh as f32 * scale).round() as u32);
            let (_, r) = rects.iter().find(|(k, _)| *k == Tray::Audio).copied().expect("sound");
            sc.pointer_button((r.0 + r.2 / 2.0) * scale, (r.1 + r.3 / 2.0) * scale, Button::Left);
            assert_eq!(sc.inner().open, Open::Popup(Tray::Audio), "{edge:?} {scale}");
            let p = sc.inner().popup.clone().unwrap();
            if edge == Edge::Top {
                assert!(p.y >= 32.0 - 0.01, "{edge:?} {scale}: the popup starts below the declared bar ({})", p.y);
            } else {
                assert!(p.y + p.h <= top + 0.01, "{edge:?} {scale}: the popup ends above the declared bar ({} > {top})", p.y + p.h);
            }
            // the input region covers the strip
            let strip = sc.input_rects();
            let (cx, cy) = ((r.0 + r.2 / 2.0) * scale, (top + 16.0) * scale);
            assert!(strip.iter().any(|i| cx >= i.x as f32 && cx < (i.x + i.w) as f32 && cy >= i.y as f32 && cy < (i.y + i.h) as f32), "{edge:?} {scale}");
        }
    }
}

#[test]
fn a_side_rail_keeps_room_for_its_cells_and_grows_with_a_larger_declaration() {
    for edge in [Edge::Left, Edge::Right] {
        for (declared, want) in [(None, 40), (Some(32.0), 40), (Some(52.0), 52)] {
            let _t = themed(declared, 1.0);
            let bar = fixture(Renderer::MenuBar, edge, AppletSet::all(), 900);
            assert_eq!(bar.surface_spec().unwrap().reserve, want, "{edge:?} {declared:?}: the rail's strut");
            assert_eq!(crate::theme::th().bar_thickness(edge) as i32, want, "{edge:?} {declared:?}: the framework agrees");
        }
    }
}

#[test]
fn scaled_text_raises_a_small_declaration_and_nothing_clips() {
    let _t = themed(Some(20.0), 1.4);
    let (reserve, pm, top, h) = drawn(Renderer::MenuBar, Edge::Top, 1.0);
    assert_eq!((reserve, h, top), (27, 27.0, 0.0));
    let (lo, hi) = ink_rows(&pm, 0, 27);
    assert!(lo >= 1 && hi + 2 <= 27, "content {lo}..{hi} inside the 27-row strip");
}

#[test]
fn an_undeclared_bar_is_the_classic_strip_at_every_scale() {
    for scale in SCALES {
        let _t = themed(None, 1.0);
        let (reserve, pm, top, h) = drawn(Renderer::MenuBar, Edge::Top, scale);
        assert_eq!((reserve, h, top), (26, 26.0, 0.0), "{scale}");
        assert_eq!(alpha(&pm, 1, 0), 255);
        assert_eq!(alpha(&pm, 1, (26.0 * scale).ceil() as u32 + 2), 0, "{scale}: nothing beyond the 26 px strip");
    }
}

#[test]
fn a_short_output_still_gets_the_whole_declared_strip() {
    for edge in [Edge::Top, Edge::Bottom] {
        let _t = themed(Some(32.0), 1.0);
        let mut bar = fixture(Renderer::MenuBar, edge, AppletSet::all(), 1024);
        bar.resized(1024, 80);
        let mut pm = Pixmap::new(1024, 80).unwrap();
        bar.draw(&mut pm);
        let first = if edge == Edge::Bottom { 80 - 32 } else { 0 };
        assert_eq!(bar.strip_top() as u32, first, "{edge:?}");
        for y in [first, first + 16, first + 31] {
            assert_eq!(alpha(&pm, 1, y), 255, "{edge:?}: row {y}");
        }
        assert!(bar.input_rects().iter().any(|r| r.h as f32 >= 32.0), "{edge:?}: an input region covers the strip");
    }
    reset();
}
