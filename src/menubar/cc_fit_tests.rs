//! The responsive Control Center: every look in every host, on short outputs at display and text scales of 100, 125 and
//! 200 %. The panel stays inside the output, the keyboard reaches every control (revealing it in the clipped viewport) and
//! the pointer reaches each one where it is drawn after the scroll, through the zoom when the look had to be zoomed.
use super::applets::tests::{fixture, reset};
use super::*;
use crate::app::NavKey;
use crate::controlcenter::settings::{Settings, Style, Tiles};
use crate::panel::{AppletSet, Edge, Renderer};

const OUTPUTS: [(f32, f32); 2] = [(1280.0, 520.0), (1024.0, 480.0)];
const SCALES: [f32; 3] = [1.0, 1.25, 2.0];
const STYLES: [Style; 4] = [Style::Card, Style::Fluent, Style::Dux, Style::Gnome];

fn theme(style: Style, text: f32) -> crate::theme::live::ThreadTheme {
    let mut t = crate::theme::builtin::def("nord").unwrap().resolve("nord", true, &crate::theme::model::Selection::default());
    t.control_center = Settings { style, tiles: Tiles::ALL, ..Settings::default() };
    t.font_scale = text;
    t.popup_alpha = 1.0;
    crate::theme::live::thread_theme(t)
}

fn snapshot() -> Snapshot {
    Snapshot {
        net: Some(sys::Net { wifi_enabled: true, wifi_name: "Home".into(), wired_name: "Wired".into() }),
        bluetooth: Some(sys::Bluetooth { powered: true, connected: 1 }),
        volume: Some(sys::Volume { level: 0.5, muted: false }),
        sink: "Headset".into(),
        brightness: Some(40),
        night_shift: Some(false),
        dnd: Some(false),
        media: Some(sys::Media { playing: true, title: "A song with a long title".into(), artist: "Artist".into() }),
        battery: Some(sys::Battery { percent: 82, charging: false, status: "Discharging".into() }),
        util: Some(sys::Utils::default()),
        lock: true,
        power: true,
        ..Snapshot::default()
    }
}

/// The panel's surface rectangle in its host.
fn panel_rect(bar: &DecBar) -> (f32, f32, f32, f32) {
    let (ox, oy) = bar.panel_origin();
    (ox, oy, bar.cc_width(), bar.cc_visible_height())
}

/// Every keyboard stop is revealed inside the visible part and the pointer, aimed where the control is drawn now (after
/// the scroll, through the zoom), reaches that control; the whole panel lies inside `bounds` (x0, y0, x1, y1).
fn check(bar: &mut DecBar, what: &str, bounds: (f32, f32, f32, f32)) {
    let (x, y, w, h) = panel_rect(bar);
    assert!(w > 0.0 && h > 0.0, "{what}: a panel ({w} x {h})");
    assert!(x >= bounds.0 - 0.5 && y >= bounds.1 - 0.5 && x + w <= bounds.2 + 0.5 && y + h <= bounds.3 + 0.5, "{what}: panel {x},{y} {w}x{h} outside {bounds:?}");
    let z = bar.cc_zoom();
    for i in &bar.cc_items {
        assert!(i.r.x >= -0.5 && i.r.x + i.r.w <= bar.cc_w + 0.5, "{what}: {:?} beyond the panel's width {}", i.r, bar.cc_w);
    }
    let order = controlcenter::targets_in_order(&bar.cc_items);
    assert!(order.len() >= 10, "{what}: {} controls", order.len());
    bar.cc_viewport.focus = None;
    for target in &order {
        assert!(crate::app::App::nav_key(bar, NavKey::Down), "{what}: Down");
        assert_eq!(bar.cc_viewport.focus, Some(*target), "{what}");
        let r = bar.cc_items.iter().find(|i| i.target == Some(*target)).unwrap().r;
        let (scroll, view) = (bar.cc_viewport.scroll, bar.cc_view_height());
        assert!(r.y - scroll >= -0.5 && r.y + r.h - scroll <= view + 0.5, "{what}: {target:?} revealed (y {} h {} scroll {scroll} view {view})", r.y, r.h);
        // the pointer where it is drawn: the middle of its visible part
        let (ox, oy) = bar.panel_origin();
        let (px, py) = (ox + (r.x + r.w / 2.0) * z, oy + (r.y - scroll + r.h / 2.0) * z);
        let at = bar.cc_point(px, py).and_then(|(cx, cy)| controlcenter::hit(&bar.cc_items, cx, cy)).map(|h| h.0);
        assert_eq!(at, Some(*target), "{what}: the pointer at ({px}, {py}) reaches {target:?}");
    }
}

#[test]
fn the_gnome_grid_reflows_to_one_column_and_keeps_every_control() {
    let s = snapshot();
    for k in SCALES {
        let (two, _) = controlcenter::gnome::layout_cols(&s, &Tiles::ALL, k, 2);
        let (one, _) = controlcenter::gnome::layout_cols(&s, &Tiles::ALL, k, 1);
        assert_eq!(controlcenter::targets_in_order(&two).len(), controlcenter::targets_in_order(&one).len(), "{k}: one column keeps every control");
        let w1 = controlcenter::gnome::panel_width_cols(k, 1);
        assert!(w1 < controlcenter::gnome::panel_width_at(k), "{k}: one column is narrower");
        for i in &one {
            assert!(i.r.x >= 0.0 && i.r.x + i.r.w <= w1 + 0.5, "{k}: {:?} inside {w1}", i.r);
            if let controlcenter::Draw::Text { text, .. } = &i.draw {
                assert!(i.r.w >= 40.0, "{k}: text `{text}` keeps room to be read ({})", i.r.w);
            }
        }
        // two columns where they fit, one where they do not, and the zoom only when even one does not
        assert_eq!(controlcenter::gnome::columns_for(controlcenter::gnome::panel_width_at(k), k), 2);
        assert_eq!(controlcenter::gnome::columns_for(w1, k), 1);
    }
}

#[test]
fn every_look_is_fitted_to_its_room_by_reflow_or_zoom() {
    let s = snapshot();
    for style in STYLES {
        for k in SCALES {
            let _t = theme(style, k);
            for room in [2000.0, 600.0, 450.0, 300.0] {
                let f = controlcenter::layout_fitted(&s, &Tiles::ALL, style, room);
                assert!(f.width * f.zoom <= room + 0.5, "{style:?} {k} {room}: {} x {}", f.width, f.zoom);
                if style == Style::Gnome && controlcenter::gnome::panel_width_cols(controlcenter::gnome::scale(), 1) <= room {
                    assert_eq!(f.zoom, 1.0, "{k} {room}: GNOME reflows before it zooms");
                }
                if f.width <= room {
                    assert_eq!(f.zoom, 1.0);
                }
            }
        }
    }
}

#[test]
fn the_menu_bar_fits_the_panel_on_short_outputs_at_every_scale() {
    let _g = tests::LOG_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    for style in STYLES {
        for k in SCALES {
            let _t = theme(style, k);
            for (ow, oh) in OUTPUTS {
                for d in SCALES {
                    let (w, h) = (ow / d, oh / d);
                    for edge in [Edge::Top, Edge::Bottom] {
                        let mut bar = fixture(Renderer::MenuBar, edge, AppletSet::all(), w as u32);
                        bar.update_snapshot(|x| *x = snapshot());
                        (bar.width, bar.height) = (w as u32, h as u32);
                        bar.open_control();
                        bar.cc_sync_viewport();
                        check(&mut bar, &format!("menu bar {edge:?} {style:?} text {k} {ow}x{oh}@{d}"), (0.0, 0.0, w, h));
                    }
                }
            }
        }
    }
    reset();
}

#[test]
fn the_rail_fits_the_panel_beside_it_on_short_outputs_at_every_scale() {
    let (mut zoomed, mut one_column) = (0, 0);
    for style in STYLES {
        for k in SCALES {
            let _t = theme(style, k);
            for (ow, oh) in OUTPUTS {
                for d in SCALES {
                    let (w, h) = (ow / d, oh / d);
                    for edge in Edge::ALL {
                        let mut bar = fixture(Renderer::Pill, edge, AppletSet::all(), if edge.is_vertical() { h as u32 } else { w as u32 });
                        bar.update_snapshot(|x| *x = snapshot());
                        // what the backend gives: the requested surface, never larger than the output
                        let p = bar.place().unwrap();
                        let (sw, sh) = bar.v_surface_size(p);
                        (bar.width, bar.height) = if edge.is_vertical() { ((sw as f32).min(w) as u32, h as u32) } else { (w as u32, (sh as f32).min(h) as u32) };
                        bar.open_control();
                        bar.cc_sync_viewport();
                        let bounds = (0.0, 0.0, bar.width as f32, bar.height as f32);
                        check(&mut bar, &format!("rail {edge:?} {style:?} text {k} {ow}x{oh}@{d}"), bounds);
                        zoomed += (bar.cc_zoom() < 1.0) as u32;
                        one_column += (style == Style::Gnome && bar.cc_w < controlcenter::gnome::panel_width_at(k)) as u32;
                    }
                }
            }
        }
    }
    // the matrix reaches both remedies: GNOME's one column, and a zoom for a look that does not fit at all
    assert!(zoomed > 0 && one_column > 0, "zoomed {zoomed}, one column {one_column}");
    reset();
}

#[test]
fn the_taskbar_engine_fits_the_panel_on_short_outputs_at_every_scale() {
    for style in STYLES {
        for k in SCALES {
            let _t = theme(style, k);
            for (ow, oh) in OUTPUTS {
                for d in SCALES {
                    let (w, h) = (ow / d, oh / d);
                    let bar_h = 48.0;
                    for edge in Edge::ALL {
                        let mut bar = DecBar::status_engine();
                        bar.update_snapshot(|x| *x = snapshot());
                        // the taskbar's surface on its output: the strip plus room for the popup, never larger than the output
                        bar.status_set_surface_size(w, h);
                        let at = if edge.is_vertical() { h - 60.0 } else { w - 60.0 };
                        assert!(bar.status_toggle(Tray::Control, at), "{style:?}");
                        match edge {
                            Edge::Bottom => bar.status_place(h - bar_h, true, w),
                            Edge::Top => bar.status_place(bar_h, false, w),
                            Edge::Left => bar.status_place_side(bar_h, false, h),
                            Edge::Right => bar.status_place_side(w - bar_h, true, h),
                        }
                        let bounds = match edge {
                            Edge::Bottom => (0.0, 0.0, w, h - bar_h),
                            Edge::Top => (0.0, bar_h, w, h),
                            Edge::Left => (bar_h, 0.0, w, h),
                            Edge::Right => (0.0, 0.0, w - bar_h, h),
                        };
                        check(&mut bar, &format!("taskbar {edge:?} {style:?} text {k} {ow}x{oh}@{d}"), bounds);
                    }
                }
            }
        }
    }
}

#[test]
fn the_summoned_overlay_fits_the_panel_on_short_outputs_at_every_scale() {
    use crate::controlcenter::settings::{place, Corner, Host};
    for style in STYLES {
        for k in SCALES {
            let _t = theme(style, k);
            for (ow, oh) in OUTPUTS {
                for d in SCALES {
                    let (w, h) = (ow / d, oh / d);
                    for (host, corner) in [
                        (Host::Nothing, Corner::Auto),
                        (Host::Panel { edge: Edge::Left, thickness: 40.0, taskbar: false }, Corner::Auto),
                        (Host::Taskbar { top: false, thickness: 48.0 }, Corner::BottomLeft),
                    ] {
                        let p = place(&host, corner);
                        let mut o = overlay::Overlay::new(p, host.is_taskbar());
                        o.bar_mut().update_snapshot(|x| *x = snapshot());
                        o.bar_mut().embed.pinned = Some((0.0, 0.0));
                        o.bar_mut().open_control();
                        crate::app::App::resized(&mut o, w as u32, h as u32);
                        let what = format!("overlay {host:?} {corner:?} {style:?} text {k} {ow}x{oh}@{d}");
                        check(o.bar_mut(), &what, (0.0, 0.0, w, h));
                    }
                }
            }
        }
    }
}

#[test]
fn a_rail_too_short_for_its_essentials_folds_them_instead_of_overlapping() {
    // a 1024 x 480 output at 200 % leaves a side rail 240 logical pixels
    for style in [Renderer::Pill, Renderer::MenuBar, Renderer::Floating, Renderer::Dux] {
        for edge in [Edge::Left, Edge::Right] {
            let bar = fixture(style, edge, AppletSet::all(), 240);
            let layout = bar.v_layout();
            let pills: Vec<_> = layout.pills.iter().map(|(_, r)| *r).collect();
            for (i, a) in pills.iter().enumerate() {
                assert!(a.y >= -0.5 && a.y + a.h <= 240.5, "{style:?} {edge:?}: pill {a:?} on the rail");
                for b in &pills[i + 1..] {
                    let overlap = a.y + 0.5 < b.y + b.h && b.y + 0.5 < a.y + a.h && a.x + 0.5 < b.x + b.w && b.x + 0.5 < a.x + a.w;
                    assert!(!overlap, "{style:?} {edge:?}: {a:?} overlaps {b:?}");
                }
            }
            assert!(layout.items.iter().any(|(k, _)| *k == Tray::Control), "{style:?} {edge:?}: the Control Center stays");
        }
    }
    reset();
}
