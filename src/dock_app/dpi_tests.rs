//! The macOS-style dock at a display scale (`Xft.dpi` / `DECEBAL_SCALE` on X11): the app stays in logical pixels and the backend's
//! [`Scaled`] wrapper makes the window, strut, input region, glass mask, drawing and pointer physical. These tests drive the dock
//! through that wrapper on every edge and check that the window, the drawn pixels and the hit-testing agree.

use super::keyboard::tests::{parts, EDGES};
use super::*;
use crate::backend::scaled::Scaled;
use crate::panel::Edge;

const SCALES: [f32; 3] = [1.0, 1.25, 2.0];
const APPS: [&str; 5] = ["launchpad", "foot", "firefox", "nautilus", "trash"];

struct Guard(PathBuf);
impl Drop for Guard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

type Log = Arc<Mutex<super::keyboard::tests::Log>>;
use std::sync::{Arc, Mutex};

fn desk(name: &str, edge: Edge, scale: f32, apps: &[&str], reduce_motion: bool) -> (Scaled<DockApp>, Log, Guard) {
    let (app, log, dir) = parts(name, edge, apps, vec![], reduce_motion);
    (Scaled::new(app, scale), log, Guard(dir))
}

/// Settles the surface the way the backend does: the window gets the size the app asks for, clamped to the output.
fn settle(a: &mut Scaled<DockApp>, output: Option<(u32, u32)>) -> (u32, u32) {
    if let Some((w, h)) = output {
        a.output_size(w, h);
    }
    let (mut w, mut h) = a.size();
    if let Some((ow, oh)) = output {
        (w, h) = (w.min(ow), h.min(oh)); // `backend::x11::place` never makes the window larger than the output
    }
    a.resized(w, h);
    (w, h)
}

fn logical_centre(a: &Scaled<DockApp>, i: usize) -> (f32, f32) {
    let d = &a.inner().dock;
    let (w, h) = a.inner().surface;
    let (len, _) = if d.edge().is_vertical() { (h as f32, w as f32) } else { (w as f32, h as f32) };
    let c = d.layout(len).cells[i];
    let (x, y, ..) = d.map(w as f32, h as f32, c.x + c.w / 2.0, d.panel_height() / 2.0, 0.0, 0.0);
    (x, y)
}

fn render(a: &mut Scaled<DockApp>, size: (u32, u32)) -> tiny_skia::Pixmap {
    let mut pm = tiny_skia::Pixmap::new(size.0, size.1).unwrap();
    a.draw(&mut pm);
    pm
}

fn alpha(pm: &tiny_skia::Pixmap, x: f32, y: f32) -> u8 {
    pm.pixel((x.round().max(0.0) as u32).min(pm.width() - 1), (y.round().max(0.0) as u32).min(pm.height() - 1)).unwrap().alpha()
}

fn bbox_of_difference(a: &tiny_skia::Pixmap, b: &tiny_skia::Pixmap) -> Option<(u32, u32, u32, u32)> {
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for y in 0..a.height() {
        for x in 0..a.width() {
            if a.pixel(x, y) != b.pixel(x, y) {
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
            }
        }
    }
    (x1 >= x0 && y1 >= y0).then_some((x0, y0, x1 + 1, y1 + 1))
}

#[test]
fn the_window_strut_input_and_glass_are_the_logical_ones_times_the_scale_on_every_edge() {
    for edge in EDGES {
        let (mut base, _log, _g) = desk("dpi-size", edge, 1.0, &APPS, false);
        let one = settle(&mut base, None);
        let spec1 = base.surface_spec().unwrap();
        let glass1 = base.glass_regions()[0];
        for s in SCALES {
            let (mut a, _log, _g) = desk("dpi-size", edge, s, &APPS, false);
            let (w, h) = settle(&mut a, None);
            assert_eq!((w, h), ((one.0 as f32 * s).ceil() as u32, (one.1 as f32 * s).ceil() as u32), "{edge:?} {s}: real window size = logical size x scale");
            assert_eq!(a.inner().surface, one, "{edge:?} {s}: the app's own surface is the logical size again");
            let spec = a.surface_spec().unwrap();
            assert_eq!((spec.reserve, spec.margin), ((spec1.reserve as f32 * s).ceil() as i32, (spec1.margin as f32 * s).round() as i32), "{edge:?} {s}: strut = reserve + margin, both physical");
            let g = a.glass_regions()[0];
            for (got, want) in [(g.x, glass1.x), (g.y, glass1.y), (g.w, glass1.w), (g.h, glass1.h), (g.radius, glass1.radius)] {
                assert!((got - want * s).abs() < 0.01, "{edge:?} {s}: the glass mask is the logical panel x scale ({got} vs {})", want * s);
            }
            // the input region is the panel strip and stays inside the window
            for r in a.input_rects() {
                assert!(r.x >= 0 && r.y >= 0 && r.x + r.w <= w as i32 && r.y + r.h <= h as i32, "{edge:?} {s}: {r:?} inside {w}x{h}");
            }
            // the strut covers exactly what the drawn panel needs: panel thickness + the 8 px of slack, physical, never more than the window across
            let across = if edge.is_vertical() { w } else { h } as i32;
            assert!(spec.reserve <= across, "{edge:?} {s}: the reserved strip fits the window ({} of {across})", spec.reserve);
        }
    }
}

#[test]
fn the_pixels_the_ring_and_the_pointer_agree_at_every_scale_on_every_edge() {
    // Pixel subtraction must isolate the ring: parallel theme publishers and
    // wall-clock launch bounces otherwise change unrelated pixels between shots.
    let theme = crate::theme::builtin::def("macos").unwrap().resolve("macos", true, &crate::theme::model::Selection::default());
    let _theme = crate::theme::live::thread_theme(theme);
    for edge in EDGES {
        let mut ring_size_at_one = None;
        for s in SCALES {
            let (mut a, log, _g) = desk("dpi-map", edge, s, &APPS, true);
            let size = settle(&mut a, None);
            let plain = render(&mut a, size);
            assert_eq!((plain.width(), plain.height()), size);
            for (i, app) in APPS.iter().enumerate() {
                // the icon is drawn under the physical point that hit-tests to it, and that point clicks exactly that item
                let (lx, ly) = logical_centre(&a, i);
                let (px, py) = (lx * s, ly * s);
                assert!(alpha(&plain, px, py) > 0, "{edge:?} {s}: item {i} has pixels at {px},{py}");
                a.pointer_button(px, py, Button::Left);
                a.pointer_release(px, py, Button::Left);
                assert_eq!(log.lock().unwrap().launched.last().map(String::as_str), Some(*app), "{edge:?} {s}: a click at {px},{py} activates item {i}");
            }
            assert!(!a.inner_mut().dock.bouncing(), "ring measurements must not include launch animation");
            // the panel's rounded plate ends where the glass mask says; outside it nothing is drawn
            let g = a.glass_regions()[0];
            let outside = match edge {
                Edge::Bottom => (g.x + g.w / 2.0, g.y - 3.0),
                Edge::Top => (g.x + g.w / 2.0, g.y + g.h + 3.0),
                Edge::Left => (g.x + g.w + 3.0, g.y + g.h / 2.0),
                Edge::Right => (g.x - 3.0, g.y + g.h / 2.0),
            };
            assert_eq!(alpha(&plain, outside.0, outside.1), 0, "{edge:?} {s}: nothing beside the panel at {outside:?}");
            assert!(alpha(&plain, g.x + g.w / 2.0, g.y + g.h / 2.0) > 0, "{edge:?} {s}: the plate is where the glass mask is");
            // the keyboard ring: on the second item, its size scales with the display, its box is inside that item's physical cell
            assert!(a.inner_mut().kb_start());
            let first = render(&mut a, size);
            assert!(a.nav_key(match edge {
                Edge::Bottom | Edge::Top => NavKey::Right,
                _ => NavKey::Down,
            }));
            let second = render(&mut a, size);
            let ring = bbox_of_difference(&plain, &second).expect("a ring");
            let (lx, ly) = logical_centre(&a, 1);
            let (cx, cy) = (lx * s, ly * s);
            assert!((ring.0 as f32..=ring.2 as f32).contains(&cx) && (ring.1 as f32..=ring.3 as f32).contains(&cy), "{edge:?} {s}: the ring {ring:?} surrounds the item at {cx},{cy}");
            assert_ne!(bbox_of_difference(&plain, &first), Some(ring), "{edge:?} {s}: the ring moved");
            let ring_w = (ring.2 - ring.0) as f32 / s;
            match ring_size_at_one {
                None => ring_size_at_one = Some(ring_w),
                Some(one) => assert!((ring_w - one).abs() < 3.0, "{edge:?} {s}: the ring is {ring_w} logical px wide, {one} at 100 %"),
            }
            a.key(Key::Escape);
        }
    }
}

#[test]
fn hover_magnification_follows_the_physical_pointer_and_reduced_motion_keeps_the_layout_still() {
    for reduce in [false, true] {
        for edge in EDGES {
            let (mut a, _log, _g) = desk("dpi-hover", edge, 2.0, &APPS, reduce);
            let size = settle(&mut a, None);
            let before: Vec<f32> = {
                let d = &a.inner().dock;
                let (w, h) = a.inner().surface;
                d.layout(if d.edge().is_vertical() { h as f32 } else { w as f32 }).cells.iter().map(|c| c.w).collect()
            };
            let (lx, ly) = logical_centre(&a, 2);
            a.pointer_motion(lx * 2.0, ly * 2.0);
            for _ in 0..40 {
                a.tick();
            }
            let after: Vec<f32> = {
                let d = &a.inner().dock;
                let (w, h) = a.inner().surface;
                d.layout(if d.edge().is_vertical() { h as f32 } else { w as f32 }).cells.iter().map(|c| c.w).collect()
            };
            if reduce {
                assert_eq!(before, after, "{edge:?}: reduced motion never magnifies");
            } else {
                assert!(after[2] > before[2] * 1.2 && after[0] < after[2], "{edge:?}: the hovered cell grows under a physical pointer position ({after:?})");
            }
            // the picture is still the right size and still hit-testable at the grown layout
            assert_eq!(render(&mut a, size).width(), size.0);
            assert!(a.inner().dock.hit_test(a.inner().surface.0 as f32, a.inner().surface.1 as f32, lx, ly).is_some());
        }
    }
}

#[test]
fn a_short_or_narrow_output_shrinks_the_dock_to_fit_and_never_exceeds_it() {
    // (output in physical px, many apps): narrow for the horizontal docks, short for the side ones
    let many: Vec<String> = (0..14).map(|i| format!("app{i}")).collect();
    let apps: Vec<&str> = many.iter().map(String::as_str).collect();
    for edge in EDGES {
        for s in SCALES {
            for output in [(1366u32, 600u32), (640, 480), (900, 520), (1920, 1080)] {
                let (mut a, log, _g) = desk("dpi-clamp", edge, s, &apps, false);
                let (w, h) = settle(&mut a, Some(output));
                assert!(w <= output.0 && h <= output.1, "{edge:?} {s} {output:?}: the window {w}x{h} fits the output");
                let along_out = if edge.is_vertical() { output.1 } else { output.0 };
                let along = if edge.is_vertical() { h } else { w };
                let natural = {
                    let (mut n, _l, _g2) = desk("dpi-clamp-n", edge, s, &apps, false);
                    let (nw, nh) = settle(&mut n, None);
                    if edge.is_vertical() {
                        nh
                    } else {
                        nw
                    }
                };
                if natural <= along_out {
                    assert_eq!(along, natural, "{edge:?} {s} {output:?}: a dock that fits is not shrunk");
                } else {
                    assert!(along <= along_out && a.inner().dock.output.is_some(), "{edge:?} {s} {output:?}: shrunk to {along} of {along_out}");
                }
                // every item stays on the surface and clickable at its physical position, and the picture draws
                let pm = render(&mut a, (w, h));
                for (i, app) in apps.iter().enumerate() {
                    let (lx, ly) = logical_centre(&a, i);
                    let (px, py) = (lx * s, ly * s);
                    if along_out >= natural || a.inner().dock.fit_for_tests() > 0.45 {
                        assert!(px >= 0.0 && py >= 0.0 && px <= w as f32 && py <= h as f32, "{edge:?} {s} {output:?}: item {i} at {px},{py} is on the {w}x{h} surface");
                        assert!(alpha(&pm, px, py) > 0, "{edge:?} {s} {output:?}: item {i} is drawn at {px},{py}");
                        a.pointer_button(px, py, Button::Left);
                        a.pointer_release(px, py, Button::Left);
                        assert_eq!(log.lock().unwrap().launched.last().map(String::as_str), Some(*app), "{edge:?} {s} {output:?}: item {i} clicks");
                    }
                }
                // the ring on the last item is still on the surface
                a.inner_mut().kb_start();
                a.home_end(true);
                let ringed = render(&mut a, (w, h));
                assert!(bbox_of_difference(&pm, &ringed).is_some(), "{edge:?} {s} {output:?}: the ring is drawn");
            }
        }
    }
}

#[test]
fn a_dock_with_far_too_many_items_still_never_exceeds_the_output() {
    let many: Vec<String> = (0..80).map(|i| format!("app{i}")).collect();
    let apps: Vec<&str> = many.iter().map(String::as_str).collect();
    for edge in EDGES {
        let (mut a, _log, _g) = desk("dpi-many", edge, 2.0, &apps, false);
        let (w, h) = settle(&mut a, Some((800, 600)));
        assert!(w <= 800 && h <= 600, "{edge:?}: {w}x{h}");
        let _ = render(&mut a, (w, h));
        assert!(a.inner().dock.fit_for_tests() >= 0.4 - f32::EPSILON, "never smaller than the floor");
    }
}

#[test]
fn the_taskbar_keeps_scaling_itself_and_the_dock_takes_the_backend_scale_only_in_the_mac_style() {
    let (app, _log, dir) = parts("dpi-style", Edge::Bottom, &APPS, vec![], false);
    let _g = Guard(dir.clone());
    let mut a = Scaled::new(app, 2.0);
    assert!(a.backend_scaled(), "the macOS dock is scaled by the backend");
    assert_eq!(a.size().0, (a.inner().size().0 as f32 * 2.0).ceil() as u32);
    std::fs::write(dir.join("settings.json"), r#"{"style":"windows"}"#).unwrap();
    a.tick();
    assert!(!a.backend_scaled(), "the taskbar applies its own scale: the wrapper must not apply it again");
    assert_eq!(a.size(), a.inner().size(), "so the window is the taskbar's own size, not doubled");
    std::fs::write(dir.join("settings.json"), r#"{"style":"mac"}"#).unwrap();
    a.tick();
    assert!(a.backend_scaled() && a.size().0 == (a.inner().size().0 as f32 * 2.0).ceil() as u32);
}

#[test]
fn switching_from_a_taskbar_to_a_mac_dock_reconverts_the_cached_physical_output() {
    for edge in EDGES {
        let (app, _log, dir) = parts("dpi-output-switch", edge, &APPS, vec![], false);
        let _g = Guard(dir.clone());
        let mut a = Scaled::new(app, 2.0);
        std::fs::write(dir.join("settings.json"), r#"{"style":"windows"}"#).unwrap();
        a.tick();
        a.output_size(640, 480);
        assert_eq!(a.inner().dock.output, Some((640.0, 480.0)));
        std::fs::write(dir.join("settings.json"), r#"{"style":"mac"}"#).unwrap();
        a.tick();
        assert_eq!(a.inner().dock.output, Some((320.0, 240.0)), "{edge:?}: cached physical output must be translated after a style change without a RandR event");
        let size = a.size();
        assert!(size.0 <= 640 && size.1 <= 480, "{edge:?}: requested dock {size:?} fits, without backend cropping");
        std::fs::write(dir.join("settings.json"), r#"{"style":"windows"}"#).unwrap();
        a.tick();
        assert_eq!(a.inner().dock.output, Some((640.0, 480.0)), "{edge:?}: the reverse transition restores physical dimensions");
    }
}

#[test]
fn menu_text_is_measured_at_the_surface_size_so_rows_fit_the_drawn_glyphs() {
    let Some(f) = Fonts::load() else { return };
    for k in [1.0f32, 1.25, 2.0] {
        let logical = crate::gfx::with_render_scale(k, || measure_logical(&f, "Show in Finder"));
        let physical = f.measure("Show in Finder", TEXT_PX * k, Weight::Regular);
        assert!((logical * k - physical).abs() < 0.01, "{k}: {logical} x {k} vs {physical}");
    }
}
