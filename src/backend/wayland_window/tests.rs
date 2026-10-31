//! Geometry and state of the Wayland window backend: card placement, output scale, damage, frame pacing, buffer
//! backpressure, the keyboard grab and the outputs a card covers. Protocol behaviour needs a compositor (see
//! docs/wayland-compact-popups.md); these pin the decisions the backend makes from what the compositor tells it.

use super::*;
use crate::window_app::{Edge, KeyEvent, MonitorHint, PopupAnchor};

/// Just enough app to place a card.
struct Stub {
    size: (u32, u32),
    anchor: Option<PopupAnchor>,
    bottom: Option<f32>,
    passive: bool,
    fullscreen: bool,
}

impl WindowApp for Stub {
    fn title(&self) -> String {
        "stub".into()
    }
    fn overlay(&self) -> bool {
        true
    }
    fn popup_anchor(&self) -> Option<PopupAnchor> {
        self.anchor
    }
    fn popup_bottom_margin(&self) -> Option<f32> {
        self.bottom
    }
    fn popup_passive(&self) -> bool {
        self.passive
    }
    fn popup_fullscreen(&self) -> bool {
        self.fullscreen
    }
    fn initial_size(&self) -> (u32, u32) {
        self.size
    }
    fn set_waker(&mut self, _: Waker) {}
    fn set_screen(&mut self, _: u32, _: u32) {}
    fn resize(&mut self, _: u32, _: u32) {}
    fn set_rounded(&mut self, _: bool) {}
    fn draw(&mut self, _: &mut Pixmap) {}
    fn pointer_motion(&mut self, _: f32, _: f32) -> Response {
        Response::NONE
    }
    fn pointer_leave(&mut self) -> Response {
        Response::NONE
    }
    fn pointer_button(&mut self, _: f32, _: f32, _: Button, _: bool) -> Response {
        Response::NONE
    }
    fn scroll(&mut self, _: f32, _: f32, _: f32) -> Response {
        Response::NONE
    }
    fn key(&mut self, _: &KeyEvent) -> Response {
        Response::NONE
    }
    fn tick(&mut self) -> Response {
        Response::NONE
    }
}

fn stub() -> Stub {
    Stub { size: (400, 300), anchor: None, bottom: None, passive: false, fullscreen: false }
}

fn top(top: i32, center_x: Option<i32>) -> PopupAnchor {
    PopupAnchor { top, center_x, edge: Edge::Top }
}

/// The card of a logical-pixel app on an output `sw` x `sh` logical.
fn popup_card(app: &Stub, sw: u32, sh: u32) -> (i32, i32, u32, u32) {
    let p = Placement { content: app.size, anchor: app.anchor, bottom: app.bottom, centered: app.popup_centered() };
    place_card(&p, sw, sh)
}

#[test]
fn a_card_under_a_bar_hangs_from_the_anchor_and_stays_on_screen() {
    let (x, y, w, h) = popup_card(&Stub { anchor: Some(top(30, Some(1000))), ..stub() }, 1920, 1080);
    assert_eq!((x, y, w, h), (800, 30, 400, 300));
    let (x, ..) = popup_card(&Stub { anchor: Some(top(30, Some(1915))), ..stub() }, 1920, 1080);
    assert_eq!(x, 1920 - 400 - 8, "kept 8 px from the right edge");
}

#[test]
fn a_hud_sits_centred_above_the_bottom_edge() {
    let (x, y, w, h) = popup_card(&Stub { size: (240, 120), bottom: Some(0.1), passive: true, ..stub() }, 1920, 1080);
    assert_eq!((x, y, w, h), (840, 1080 - 120 - 108, 240, 120));
}

#[test]
fn a_card_larger_than_a_small_output_is_clamped_not_overflowing() {
    let (x, y, w, h) = popup_card(&Stub { size: (4000, 4000), bottom: Some(0.05), ..stub() }, 800, 600);
    assert_eq!((w, h), (760, 600 - 30 - 8), "the bottom margin and the edge gap are kept free");
    assert!(x >= 0 && y >= 8 && x + w as i32 <= 800 && y + h as i32 <= 600 - 30, "{x},{y},{w},{h}");
}

#[test]
fn a_card_hung_low_from_a_tall_bar_is_only_as_tall_as_the_room_below_it() {
    // A bar offset of 900 on a 1080 output leaves 172 px (an 8 px gap kept at the bottom).
    let (_, y, _, h) = popup_card(&Stub { size: (400, 600), anchor: Some(top(900, None)), ..stub() }, 1920, 1080);
    assert_eq!((y, h), (900, 172));
    assert!(y + h as i32 <= 1080 - EDGE_GAP);
    // The same from a bottom bar: the card ends `top` above the bottom edge and starts 8 px below the top.
    let bottom_bar = PopupAnchor { top: 900, center_x: None, edge: Edge::Bottom };
    let (_, y, _, h) = popup_card(&Stub { size: (400, 600), anchor: Some(bottom_bar), ..stub() }, 1920, 1080);
    assert_eq!((y, h), (8, 172));
    // An offset past the output still leaves a 1 px card, never a negative or wrapping size.
    let (_, _, _, h) = popup_card(&Stub { anchor: Some(top(5000, None)), ..stub() }, 1920, 1080);
    assert_eq!(h, 1);
}

#[test]
fn a_card_beside_a_side_bar_is_only_as_wide_as_the_room_beside_it() {
    let left = PopupAnchor { top: 1500, center_x: Some(500), edge: Edge::Left };
    let (x, y, w, h) = popup_card(&Stub { size: (800, 300), anchor: Some(left), ..stub() }, 1920, 1080);
    assert_eq!((x, w), (1500, 1920 - 1500 - 8), "`top` from the left edge, 8 px kept free at the far edge");
    assert_eq!((y, h), (350, 300));
}

#[test]
fn only_cards_and_huds_are_compact_popups() {
    assert!(!stub().wants_compact_popup(), "a plain overlay (Spotlight, the switcher) keeps the full-screen path");
    assert!(Stub { passive: true, ..stub() }.wants_compact_popup());
    assert!(Stub { bottom: Some(0.1), ..stub() }.wants_compact_popup());
    assert!(Stub { anchor: Some(top(0, None)), ..stub() }.wants_compact_popup());
    assert!(!Stub { anchor: Some(top(0, None)), fullscreen: true, ..stub() }.wants_compact_popup(), "a full-screen popup stays full-screen");
}

#[test]
fn the_probe_asks_for_what_each_kind_of_window_needs() {
    let all = Support::from_interfaces(["wl_compositor", "wl_shm", "xdg_wm_base", "zwlr_layer_shell_v1", "wl_subcompositor", "wp_viewporter"]);
    assert!(all.runs(&stub()) && all.runs(&Stub { passive: true, ..stub() }));
    assert!(!all.fractional_scale && all.viewporter);
    let no_sub = Support { subcompositor: false, ..all };
    assert!(!no_sub.runs(&Stub { passive: true, ..stub() }), "a card is a subsurface of its host");
    assert!(no_sub.runs(&stub()), "a full-screen overlay or window does not need subsurfaces");
    let layer_only = Support { xdg_shell: false, ..all };
    assert!(layer_only.runs(&Stub { bottom: Some(0.1), ..stub() }), "a card never needs xdg-shell");
    assert!(!layer_only.runs(&stub()), "anything else falls back to an xdg window");
    assert!(!Support::default().runs(&stub()));
}

// --- scale ---------------------------------------------------------------------------------------------------

#[test]
fn a_fractional_scale_needs_a_viewport_to_be_presented() {
    assert_eq!(effective_scale(2, Some(180), true), OutputScale::Fractional(180));
    assert_eq!(effective_scale(2, Some(180), false), OutputScale::Integer(2), "without wp_viewporter the integer scale is used");
    assert_eq!(effective_scale(1, None, true), OutputScale::Integer(1));
    assert_eq!(effective_scale(0, Some(0), true), OutputScale::Integer(1), "nonsense values never produce a zero scale");
}

#[test]
fn an_applied_card_at_a_fractional_scale_has_a_logical_surface_and_a_device_buffer() {
    // 1.25: a 240 x 148 logical card is a 300 x 185 buffer presented through a viewport, at buffer scale 1.
    let g = card_geometry(PopupScale::Applied, OutputScale::Fractional(150), (240, 148));
    assert_eq!(g, CardGeom { buffer: (300, 185), buffer_scale: 1, viewport: Some((240, 148)) });
    // The device-pixel content the app reports comes back to the same logical size.
    assert_eq!((to_logical(PopupScale::Applied, OutputScale::Fractional(150), 300), to_logical(PopupScale::Applied, OutputScale::Fractional(150), 185)), (240, 148));
}

#[test]
fn an_applied_card_at_an_integer_scale_uses_the_buffer_scale() {
    let g = card_geometry(PopupScale::Applied, OutputScale::Integer(2), (240, 148));
    assert_eq!(g, CardGeom { buffer: (480, 296), buffer_scale: 2, viewport: None });
    // An odd device size rounds the logical size up, so the buffer is an exact multiple of the scale.
    assert_eq!(to_logical(PopupScale::Applied, OutputScale::Integer(2), 301), 151);
    assert_eq!(to_app(PopupScale::Applied, OutputScale::Integer(2), 151) % 2, 0);
}

#[test]
fn a_logical_card_is_drawn_at_1x_whatever_the_output_scale() {
    for s in [OutputScale::Integer(1), OutputScale::Integer(2), OutputScale::Fractional(150)] {
        assert_eq!(card_geometry(PopupScale::Logical, s, (500, 400)), CardGeom { buffer: (500, 400), buffer_scale: 1, viewport: None }, "{s:?}");
        assert_eq!(to_logical(PopupScale::Logical, s, 500), 500);
    }
}

#[test]
fn the_hud_keeps_its_logical_size_at_every_output_scale_no_double_scaling() {
    // The HUD lays itself out at the scale it is given; the logical card must stay the 1x card (within rounding),
    // not grow by Xft.dpi on top of the compositor's scale.
    let base = crate::hud::layout(1.0);
    for v in [120u32, 150, 180, 240] {
        let s = OutputScale::Fractional(v);
        let l = crate::hud::layout(s.factor() as f32);
        let (lw, lh) = (to_logical(PopupScale::Applied, s, l.w), to_logical(PopupScale::Applied, s, l.h));
        assert!(lw.abs_diff(base.w) <= 1 && lh.abs_diff(base.h) <= 1, "{v}/120: {lw}x{lh} vs {}x{}", base.w, base.h);
    }
    for n in [1, 2, 3] {
        let l = crate::hud::layout(n as f32);
        let lw = to_logical(PopupScale::Applied, OutputScale::Integer(n), l.w);
        assert!(lw.abs_diff(base.w) <= 1, "x{n}: {lw} vs {}", base.w);
    }
}

#[test]
fn an_applied_apps_anchor_is_turned_into_logical_pixels() {
    let a = PopupAnchor { top: 60, center_x: Some(1500), edge: Edge::Top };
    assert_eq!(logical_anchor(a, PopupScale::Applied, OutputScale::Integer(2)), PopupAnchor { top: 30, center_x: Some(750), edge: Edge::Top });
    assert_eq!(logical_anchor(a, PopupScale::Applied, OutputScale::Fractional(150)), PopupAnchor { top: 48, center_x: Some(1200), edge: Edge::Top });
    assert_eq!(logical_anchor(a, PopupScale::Logical, OutputScale::Integer(2)), a, "a logical app's anchor is already logical");
}

#[test]
fn the_screen_is_the_outputs_logical_size_never_a_physical_mode() {
    use wl_output::Transform as T;
    // A 4K panel at 1.5 (xdg-output reports 2560 x 1440): the mode never leaks through.
    assert_eq!(logical_output_size(Some((2560, 1440)), Some((3840, 2160)), T::Normal, 2), Some((2560, 1440)));
    // No xdg-output: the mode divided by the integer scale, turned by the transform.
    assert_eq!(logical_output_size(None, Some((3840, 2160)), T::Normal, 2), Some((1920, 1080)));
    assert_eq!(logical_output_size(None, Some((1920, 1080)), T::_90, 1), Some((1080, 1920)));
    assert_eq!(logical_output_size(Some((0, 0)), Some((1920, 1080)), T::Flipped270, 1), Some((1080, 1920)), "an empty logical size is ignored");
    assert_eq!(logical_output_size(None, None, T::Normal, 1), None);
}

// --- outputs and input -------------------------------------------------------------------------------------

fn outputs() -> Vec<OutputDesc> {
    vec![
        OutputDesc { name: Some("eDP-1".into()), rect: Some((0, 0, 1920, 1200)) },
        OutputDesc { name: Some("DP-2".into()), rect: Some((1920, -200, 2560, 1440)) },
        OutputDesc { name: None, rect: None },
    ]
}

#[test]
fn the_hint_picks_the_output_by_name_then_by_point_else_leaves_it_to_the_compositor() {
    let outs = outputs();
    assert_eq!(pick_output(&outs, &MonitorHint { output: Some("DP-2".into()), point: Some((10, 10)) }), Some(1), "the name wins over the point");
    assert_eq!(pick_output(&outs, &MonitorHint { output: None, point: Some((2000, -100)) }), Some(1), "a point on the second monitor (above the first)");
    assert_eq!(pick_output(&outs, &MonitorHint { output: Some("HDMI-9".into()), point: Some((5, 5)) }), Some(0), "an unknown name falls back to the point");
    assert_eq!(pick_output(&outs, &MonitorHint { output: None, point: Some((1920, 1199)) }), Some(1), "edges belong to the output starting there");
    assert_eq!(pick_output(&outs, &MonitorHint::default()), None, "no hint: the compositor's focused output, not the first one");
    assert_eq!(pick_output(&outs, &MonitorHint { output: None, point: Some((-5, 5000)) }), None);
}

#[test]
fn every_other_output_gets_one_catcher_once_the_cards_output_is_known() {
    let outs = ["A", "B", "C"];
    assert_eq!(missing_catchers(&outs, Some(&"B"), &[], true), vec!["A", "C"]);
    assert_eq!(missing_catchers(&outs, Some(&"B"), &["A"], true), vec!["C"], "no duplicate when an output is plugged in");
    assert!(missing_catchers(&outs, None, &[], true).is_empty(), "wait for the host to enter its output");
    assert!(missing_catchers(&outs, Some(&"B"), &[], false).is_empty(), "a card that keeps its grab on purpose does not cover the desktop");
    assert!(missing_catchers(&["A"], Some(&"A"), &[], true).is_empty(), "one monitor: the host alone catches outside clicks");
}

#[test]
fn a_click_outside_closes_only_an_interactive_popup() {
    assert!(outside_click_closes(false, true, false));
    assert!(!outside_click_closes(false, true, true), "a click on the card is the app's");
    assert!(!outside_click_closes(true, true, false), "a passive HUD takes no input");
    assert!(!outside_click_closes(false, false, false), "apps that keep the grab on purpose stay open");
}

#[test]
fn losing_the_keyboard_closes_a_card_that_had_it() {
    assert!(focus_leave_closes(true, false, true));
    assert!(!focus_leave_closes(false, false, true), "a leave without a prior enter is not a focus change");
    assert!(!focus_leave_closes(true, true, true), "a passive HUD never has the keyboard");
    assert!(!focus_leave_closes(true, false, false));
}

#[test]
fn the_keyboard_grab_is_reported_once_and_truthfully() {
    let t0 = Instant::now();
    // Granted: enter, then the modifiers arrive.
    let mut g = Grab::waiting();
    assert_eq!(g.wait(t0), None, "no clock before the first frame");
    g.mapped(t0);
    g.entered();
    assert_eq!(g.modifiers(0x8), Some(Some(0x8)), "Alt held while it opened");
    assert_eq!(g.modifiers(0), None);
    assert_eq!(g.poll(t0 + GRAB_WAIT * 2, 0), None, "reported once");
    // Refused: no enter within the wait.
    let mut g = Grab::waiting();
    g.mapped(t0);
    assert_eq!(g.poll(t0 + GRAB_WAIT / 2, 0), None);
    assert_eq!(g.wait(t0 + GRAB_WAIT / 2), Some(GRAB_WAIT / 2));
    assert_eq!(g.poll(t0 + GRAB_WAIT, 0), Some(None), "the app learns it has no keyboard");
    g.entered();
    assert_eq!(g.modifiers(0x4), None, "a late enter after the report is not reported again");
    // Entered but the compositor sent no modifiers: the current ones are reported when the wait ends.
    let mut g = Grab::waiting();
    g.entered();
    g.mapped(t0);
    assert_eq!(g.poll(t0 + GRAB_WAIT, 0x1), Some(Some(0x1)));
    // Modifiers before any enter are not a grab.
    let mut g = Grab::waiting();
    assert_eq!(g.modifiers(0x4), None);
    assert_eq!(Grab::Off.wait(t0), None);
}

#[test]
fn wayland_modifiers_map_to_the_x_mask() {
    let m = Modifiers { shift: true, ctrl: true, alt: true, logo: true, ..Modifiers::default() };
    assert_eq!(x_mods(&m), 0x1 | 0x4 | 0x8 | 0x40);
    assert_eq!(x_mods(&Modifiers::default()), 0);
    assert_eq!(x_mods(&Modifiers { caps_lock: true, num_lock: true, ..Modifiers::default() }), 0x2 | 0x10);
}

// --- buffers, damage and pacing ----------------------------------------------------------------------------

#[test]
fn a_transparent_backdrop_never_costs_a_full_output_buffer() {
    assert_eq!(transparent_plan(3840, 2160, true), (1, 1, 1), "1 x 1 stretched by the viewport");
    assert_eq!(transparent_plan(1920, 1080, false), (16, 9, 120));
    assert_eq!(transparent_plan(2560, 1440, false), (16, 9, 160));
    let (w, h, s) = transparent_plan(1366, 768, false);
    assert_eq!((w * s as u32, h * s as u32), (1366, 768), "the buffer scale stretches it exactly");
    assert_eq!(transparent_plan(0, 0, false), (1, 1, 1));
}

#[test]
fn only_the_changed_rectangle_is_damaged() {
    let (w, h) = (8u32, 6u32);
    let prev = vec![0u8; (w * h * 4) as usize];
    assert_eq!(diff_rect(&prev, &prev, w, h), None, "an unchanged frame is not committed");
    let mut cur = prev.clone();
    let px = |x: u32, y: u32| ((y * w + x) * 4) as usize;
    cur[px(3, 2) + 3] = 255;
    assert_eq!(diff_rect(&prev, &cur, w, h), Some(Damage { x: 3, y: 2, w: 1, h: 1 }));
    cur[px(6, 4)] = 9;
    cur[px(1, 3) + 1] = 9;
    assert_eq!(diff_rect(&prev, &cur, w, h), Some(Damage { x: 1, y: 2, w: 6, h: 3 }), "the bounding box of every change");
    assert_eq!(diff_rect(&prev[..4], &cur, w, h), Some(Damage::full(w, h)), "a size change damages everything");
}

#[test]
fn a_reused_buffer_catches_up_on_every_frame_it_missed() {
    let full = Damage::full(100, 100);
    let mut d = SlotDamage::default();
    let a = d.add_slot(full);
    let d1 = Damage { x: 10, y: 10, w: 5, h: 5 };
    d.frame(d1);
    assert_eq!(d.take(a), Some(full), "a new buffer is written whole");
    let b = d.add_slot(full);
    let d2 = Damage { x: 50, y: 0, w: 10, h: 2 };
    d.frame(d2);
    assert_eq!(d.take(b), Some(full));
    let d3 = Damage { x: 0, y: 90, w: 1, h: 1 };
    d.frame(d3);
    assert_eq!(d.take(a), Some(d2.union(d3)), "buffer a missed frame 2 and needs frame 3");
    assert_eq!(d.take(a), None, "up to date now");
    d.clear();
    assert_eq!(d.take(a), None, "a resize forgets every buffer");
}

#[test]
fn copying_a_rectangle_swizzles_only_that_rectangle() {
    let w = 4u32;
    let src: Vec<u8> = (0..(w * 2 * 4) as u8).collect();
    let mut dst = vec![0xAAu8; src.len()];
    copy_rect(&src, &mut dst, w, Damage { x: 1, y: 1, w: 2, h: 1 });
    let px = |x: usize, y: usize| (y * w as usize + x) * 4;
    assert_eq!(&dst[px(1, 1)..px(1, 1) + 4], &[src[px(1, 1) + 2], src[px(1, 1) + 1], src[px(1, 1)], src[px(1, 1) + 3]], "RGBA becomes BGRA");
    assert_eq!(&dst[px(0, 1)..px(0, 1) + 4], &[0xAA; 4], "outside the rectangle untouched");
    assert_eq!(&dst[px(1, 0)..px(1, 0) + 4], &[0xAA; 4]);
    // A rectangle running past the buffer stops instead of panicking.
    copy_rect(&src, &mut dst, w, Damage { x: 0, y: 1, w: 4, h: 5 });
}

#[test]
fn frames_wait_for_the_callback_and_a_lost_one_only_stalls_briefly() {
    let t0 = Instant::now();
    let mut p = FramePacer::default();
    assert!(p.can_draw(t0) && p.wait(t0).is_none());
    p.committed(t0);
    assert!(!p.can_draw(t0 + Duration::from_millis(16)), "no second frame before the compositor asks");
    assert_eq!(p.wait(t0 + Duration::from_millis(400)), Some(FRAME_STALL - Duration::from_millis(400)));
    p.done();
    assert!(p.can_draw(t0 + Duration::from_millis(17)));
    p.committed(t0);
    assert!(p.can_draw(t0 + FRAME_STALL), "a compositor that drops callbacks cannot freeze the card for good");
}

#[test]
fn at_most_three_card_buffers_exist_and_a_busy_set_waits() {
    assert_eq!(pick_slot(&[], MAX_CARD_BUFFERS), SlotChoice::New);
    assert_eq!(pick_slot(&[false, true], MAX_CARD_BUFFERS), SlotChoice::Reuse(1));
    assert_eq!(pick_slot(&[false, false], MAX_CARD_BUFFERS), SlotChoice::New, "a third buffer while two are on screen");
    assert_eq!(pick_slot(&[false; MAX_CARD_BUFFERS], MAX_CARD_BUFFERS), SlotChoice::Busy, "backpressure: never a fourth");
    assert_eq!(pick_slot(&[true, true, true], MAX_CARD_BUFFERS), SlotChoice::Reuse(0));
}

#[test]
fn mouse_buttons_map() {
    assert_eq!(button_of(BTN_LEFT), Some(Button::Left));
    assert_eq!(button_of(BTN_RIGHT), Some(Button::Right));
    assert_eq!(button_of(BTN_MIDDLE), Some(Button::Middle));
    assert_eq!(button_of(0x113), None, "side buttons are ignored");
}
