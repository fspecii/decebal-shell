//! Display-server-free helpers of the Wayland front end: output scale, where the compositor leaves room for the
//! stack, input-region conversion, wheel accumulation and key repeat. Kept apart from [`super::wl`] so they are
//! compiled and tested in every feature combination.

use super::view::{stack_origin, Btn, Metrics};
use std::time::{Duration, Instant};

/// The scale a surface is presented at: the output's integer buffer scale, or a fractional one in 120ths
/// (wp-fractional-scale-v1, presented through a wp_viewport).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scale {
    Integer(i32),
    Fractional(u32),
}

impl Scale {
    pub fn factor(self) -> f32 {
        match self {
            Scale::Integer(n) => n.max(1) as f32,
            Scale::Fractional(v) => v.max(120) as f32 / 120.0,
        }
    }

    /// Logical surface size of a device-pixel extent (at least 1).
    pub fn logical(self, device: u32) -> u32 {
        match self {
            Scale::Integer(n) => device.div_ceil(n.max(1) as u32).max(1),
            Scale::Fractional(v) => ((u64::from(device) * 120 + u64::from(v.max(120)) / 2) / u64::from(v.max(120))).max(1) as u32,
        }
    }

    /// A signed offset in device pixels as logical pixels, rounded.
    pub fn logical_i(self, device: i32) -> i32 {
        (device as f32 / self.factor()).round() as i32
    }

    /// Snaps a device-pixel size to a buffer that fills a whole number of logical pixels exactly: a multiple of an
    /// integer scale, or `round(logical * scale)` for a fractional one (what the protocol expects). Idempotent.
    pub fn fit(self, w: u32, h: u32) -> (u32, u32) {
        let one = |v: u32| match self {
            Scale::Integer(n) => v.max(1).div_ceil(n.max(1) as u32) * n.max(1) as u32,
            Scale::Fractional(f) => {
                let f = u64::from(f.max(120));
                let l = self.logical(v);
                ((u64::from(l) * f + 60) / 120).max(1) as u32
            }
        };
        (one(w), one(h))
    }
}

/// The fractional scale when the compositor sent one and a viewport can present it, else the integer one.
pub fn effective_scale(integer: i32, fractional: Option<u32>, viewporter: bool) -> Scale {
    match fractional {
        Some(v) if viewporter && v > 0 => Scale::Fractional(v),
        _ => Scale::Integer(integer.max(1)),
    }
}

/// An output's size in logical pixels: xdg-output's logical size, else the current mode turned by the transform and
/// divided by the integer scale. Never the raw physical mode.
pub fn logical_output_size(logical: Option<(i32, i32)>, mode: Option<(i32, i32)>, swap_axes: bool, scale: i32) -> Option<(u32, u32)> {
    if let Some((w, h)) = logical.filter(|&(w, h)| w > 0 && h > 0) {
        return Some((w as u32, h as u32));
    }
    let (w, h) = mode.filter(|&(w, h)| w > 0 && h > 0)?;
    let (w, h) = if swap_axes { (h, w) } else { (w, h) };
    let s = scale.max(1);
    Some(((w / s).max(1) as u32, (h / s).max(1) as u32))
}

/// Where the compositor leaves room for the stack, and how the margins are derived.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsetPolicy {
    /// The layer surfaces ask for exclusive zone 0, so the compositor keeps them clear of every other layer's exclusive
    /// zone (decbar, a pill bar, waybar...). Margins are only the visual gaps; the theme's bar sizes merely bound the
    /// stack's height and width.
    Zones,
    /// The compositor or the bar does not reserve its space: add the theme's bar insets to the margins like the X11
    /// front end does.
    Theme,
}

impl InsetPolicy {
    /// `DECNOTIFY_WAYLAND_INSETS=theme`; anything else (or nothing) is [`InsetPolicy::Zones`].
    pub fn from_env(value: Option<&str>) -> InsetPolicy {
        match value.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
            Some("theme") => InsetPolicy::Theme,
            _ => InsetPolicy::Zones,
        }
    }
}

/// The theme's bar extents in logical pixels, as the stack placement uses them.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BarFacts {
    /// Top-edge extent (`theme::bar_bottom()`).
    pub bar_bottom: f32,
    /// The Caelestia pill bar is in use.
    pub pill: bool,
    pub right: i32,
    pub bottom: i32,
}

/// Metrics for an output of `out` device pixels at scale `s`.
pub fn wayland_metrics(policy: InsetPolicy, s: f32, out: (i32, i32), bar: BarFacts) -> Metrics {
    match policy {
        InsetPolicy::Theme => {
            let (top, right_gap) = stack_origin(s, 0, out.1, bar.bar_bottom, bar.pill, bar.right);
            Metrics { s, x: 0, y: 0, w: out.0, h: out.1, top, right_gap }
        }
        InsetPolicy::Zones => {
            let px = |v: f32| (v * s).round() as i32;
            let w = (out.0 - px(bar.right as f32)).max(1);
            let h = (out.1 - px(bar.bar_bottom) - px(bar.bottom as f32)).max(1);
            let (top, right_gap) = stack_origin(s, 0, h, 0.0, bar.pill, 0);
            Metrics { s, x: 0, y: 0, w, h, top, right_gap }
        }
    }
}

/// Layer-shell margins `(top, right)` in logical pixels for a surface of `size` device pixels placed at (`x`, `y`)
/// inside `m`'s area (anchored top and right).
pub fn margins(scale: Scale, m: &Metrics, x: i32, y: i32, size: (u32, u32)) -> (i32, i32) {
    (scale.logical_i((y - m.y).max(0)), scale.logical_i((m.x + m.w - x - size.0 as i32).max(0)))
}

/// Rectangles in device pixels as logical `(x, y, w, h)` regions that cover them (never smaller).
pub fn logical_rects(rects: &[(i32, i32, u32, u32)], scale: Scale) -> Vec<(i32, i32, i32, i32)> {
    let f = scale.factor();
    rects
        .iter()
        .map(|&(x, y, w, h)| {
            let (x0, y0) = ((x as f32 / f).floor() as i32, (y as f32 / f).floor() as i32);
            let (x1, y1) = (((x + w as i32) as f32 / f).ceil() as i32, ((y + h as i32) as f32 / f).ceil() as i32);
            (x0, y0, (x1 - x0).max(1), (y1 - y0).max(1))
        })
        .collect()
}

pub fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

/// Buffer size and scale of a fully transparent surface `w` x `h` logical: 1 x 1 stretched by a viewport, else the
/// smallest buffer an integer buffer scale stretches exactly (`w / g` x `h / g` at scale `g = gcd(w, h)`).
pub fn transparent_plan(w: u32, h: u32, viewporter: bool) -> (u32, u32, i32) {
    if viewporter {
        return (1, 1, 1);
    }
    let g = gcd(w.max(1), h.max(1)).clamp(1, i32::MAX as u32);
    (w.max(1) / g, h.max(1) / g, g as i32)
}

/// A pointer button code (linux input) as the stack's button.
pub fn button_of(code: u32) -> Option<Btn> {
    match code {
        0x110 => Some(Btn::Left),
        0x111 => Some(Btn::Right),
        0x112 => Some(Btn::Middle),
        _ => None,
    }
}

/// Wheel input in whole steps (positive = down). Notched wheels report discrete steps; touchpads and smooth wheels
/// report pixels, which accumulate until they make a step.
#[derive(Debug, Default)]
pub struct ScrollAccum {
    acc: f32,
}

/// Surface pixels of smooth scrolling that make one step.
pub const SCROLL_STEP: f32 = 20.0;

impl ScrollAccum {
    pub fn push(&mut self, discrete: i32, absolute: f64) -> i32 {
        if discrete != 0 {
            self.acc = 0.0;
            return discrete;
        }
        self.acc += absolute as f32;
        let steps = (self.acc / SCROLL_STEP).trunc();
        self.acc -= steps * SCROLL_STEP;
        steps as i32
    }

    pub fn reset(&mut self) {
        self.acc = 0.0;
    }
}

/// Client-side key repeat (the compositor only sends the rate and delay).
#[derive(Debug)]
pub struct KeyRepeat {
    /// `(delay, interval)`; `None` when the compositor disabled repeat.
    info: Option<(Duration, Duration)>,
    held: Option<Held>,
}

#[derive(Debug, Clone, Copy)]
struct Held {
    raw: u32,
    sym: u32,
    next: Instant,
}

impl Default for KeyRepeat {
    fn default() -> Self {
        // what most compositors announce; used until (unless) the compositor says otherwise
        KeyRepeat { info: Some((Duration::from_millis(400), Duration::from_millis(40))), held: None }
    }
}

/// Modifier keysyms never repeat.
fn is_modifier(sym: u32) -> bool {
    (0xffe1..=0xffee).contains(&sym) || sym == 0xfe03 || sym == 0xff7f
}

impl KeyRepeat {
    /// `rate` repeats per second after `delay_ms`; a rate of 0 disables repeat.
    pub fn set_info(&mut self, rate: u32, delay_ms: u32) {
        self.info = (rate > 0).then(|| (Duration::from_millis(u64::from(delay_ms)), Duration::from_micros(1_000_000 / u64::from(rate))));
        if self.info.is_none() {
            self.held = None;
        }
    }

    pub fn disable(&mut self) {
        self.info = None;
        self.held = None;
    }

    /// A key went down: it is the one that repeats (a newer press replaces an older one).
    pub fn press(&mut self, raw: u32, sym: u32, now: Instant) {
        self.held = match self.info {
            Some((delay, _)) if !is_modifier(sym) => Some(Held { raw, sym, next: now + delay }),
            _ => None,
        };
    }

    pub fn release(&mut self, raw: u32) {
        if self.held.is_some_and(|h| h.raw == raw) {
            self.held = None;
        }
    }

    pub fn clear(&mut self) {
        self.held = None;
    }

    /// The keysyms to repeat now (at most one: a long stall does not replay the backlog).
    pub fn due(&mut self, now: Instant) -> Option<u32> {
        let (_, interval) = self.info?;
        let held = self.held.as_mut()?;
        if held.next > now {
            return None;
        }
        held.next = now + interval;
        Some(held.sym)
    }

    /// Time until the next repeat.
    pub fn wait(&self, now: Instant) -> Option<Duration> {
        self.held.map(|h| h.next.saturating_duration_since(now))
    }
}

/// The output named by `want`, else the first one; `None` when there are none.
pub fn pick_output<T: Clone>(outputs: &[(Option<String>, T)], want: Option<&str>) -> Option<T> {
    want.and_then(|w| outputs.iter().find(|(name, _)| name.as_deref() == Some(w))).or_else(|| outputs.first()).map(|(_, o)| o.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_snaps_to_whole_logical_pixels_and_is_idempotent() {
        for scale in [Scale::Integer(1), Scale::Integer(2), Scale::Integer(3), Scale::Fractional(120), Scale::Fractional(150), Scale::Fractional(180), Scale::Fractional(210), Scale::Fractional(300)] {
            for w in 1..=900u32 {
                let (fw, fh) = scale.fit(w, w + 7);
                assert!(fw >= w.saturating_sub(2) && fh + 2 >= w + 7, "{scale:?} {w}: {fw}x{fh} stays close to the request");
                assert_eq!(scale.fit(fw, fh), (fw, fh), "{scale:?} {w}");
                if let Scale::Integer(n) = scale {
                    assert_eq!(fw % n as u32, 0);
                    assert_eq!(scale.logical(fw) * n as u32, fw);
                }
            }
        }
        // 344 px at 1.5x is 229.33 logical: 229 logical pixels, a 344 px (round(229 * 1.5)) buffer
        assert_eq!(Scale::Fractional(180).logical(344), 229);
        assert_eq!(Scale::Fractional(180).fit(344, 102), (344, 102), "102 px is exactly 68 logical pixels at 1.5x");
        assert_eq!(Scale::Integer(2).fit(345, 101), (346, 102));
    }

    #[test]
    fn scale_factor_and_logical_rounding() {
        assert_eq!(Scale::Integer(0).factor(), 1.0, "a broken scale never divides by zero");
        assert_eq!(Scale::Fractional(0).factor(), 1.0);
        assert_eq!(Scale::Fractional(150).factor(), 1.25);
        assert_eq!(Scale::Integer(2).logical_i(25), 13);
        assert_eq!(Scale::Integer(2).logical_i(-3), -2, "halves round away from zero");
    }

    #[test]
    fn a_fractional_scale_needs_a_viewport() {
        assert_eq!(effective_scale(2, Some(180), true), Scale::Fractional(180));
        assert_eq!(effective_scale(2, Some(180), false), Scale::Integer(2), "no viewporter: the integer scale");
        assert_eq!(effective_scale(2, None, true), Scale::Integer(2));
        assert_eq!(effective_scale(0, Some(0), true), Scale::Integer(1));
    }

    #[test]
    fn output_size_is_logical_never_the_raw_mode() {
        assert_eq!(logical_output_size(Some((1280, 720)), Some((2560, 1440)), false, 2), Some((1280, 720)));
        assert_eq!(logical_output_size(None, Some((2560, 1440)), false, 2), Some((1280, 720)));
        assert_eq!(logical_output_size(None, Some((2560, 1440)), true, 2), Some((720, 1280)), "a rotated output swaps axes");
        assert_eq!(logical_output_size(Some((0, 0)), Some((1920, 1080)), false, 1), Some((1920, 1080)));
        assert_eq!(logical_output_size(None, None, false, 1), None);
    }

    fn bar() -> BarFacts {
        BarFacts { bar_bottom: 26.0, pill: false, right: 0, bottom: 0 }
    }

    #[test]
    fn zones_policy_leaves_vertical_room_to_the_compositor_and_theme_policy_adds_the_bar() {
        let zones = wayland_metrics(InsetPolicy::Zones, 1.0, (1920, 1080), bar());
        assert_eq!((zones.top, zones.right_gap), (8, 12), "only the visual gaps: exclusive zone 0 already clears the bar");
        assert_eq!(zones.h, 1080 - 26, "the height the stack may use excludes the bar");
        assert_eq!(zones.available_height(), (1080 - 26 - 8 - 12) as f32);
        let theme = wayland_metrics(InsetPolicy::Theme, 1.0, (1920, 1080), bar());
        assert_eq!((theme.top, theme.right_gap, theme.h), (34, 12, 1080), "the X11 placement");
        // a right pill bar: the compositor clears it in zones mode, the margin carries it in theme mode
        let pill = BarFacts { bar_bottom: 0.0, pill: true, right: 56, bottom: 0 };
        let z = wayland_metrics(InsetPolicy::Zones, 1.0, (1920, 1080), pill);
        assert_eq!((z.w, z.right_gap, z.top), (1920 - 56, 12, 12));
        let t = wayland_metrics(InsetPolicy::Theme, 1.0, (1920, 1080), pill);
        assert_eq!((t.w, t.right_gap), (1920, 12 + 56));
        // a bottom bar only shortens the room
        let bottom = BarFacts { bar_bottom: 0.0, pill: false, right: 0, bottom: 40 };
        assert_eq!(wayland_metrics(InsetPolicy::Zones, 1.0, (1920, 1080), bottom).h, 1040);
    }

    #[test]
    fn metrics_scale_with_the_output_and_stay_positive_on_tiny_outputs() {
        let m = wayland_metrics(InsetPolicy::Zones, 2.0, (3840, 2160), bar());
        assert_eq!((m.top, m.right_gap), (16, 24));
        assert_eq!(m.h, 2160 - 52);
        let tiny = wayland_metrics(InsetPolicy::Zones, 1.0, (10, 10), BarFacts { bar_bottom: 100.0, pill: false, right: 100, bottom: 100 });
        assert!(tiny.w >= 1 && tiny.h >= 1);
        assert!(tiny.available_height() >= 1.0);
    }

    #[test]
    fn policy_is_read_from_the_environment_value() {
        assert_eq!(InsetPolicy::from_env(None), InsetPolicy::Zones);
        assert_eq!(InsetPolicy::from_env(Some("Theme ")), InsetPolicy::Theme);
        assert_eq!(InsetPolicy::from_env(Some("zones")), InsetPolicy::Zones);
        assert_eq!(InsetPolicy::from_env(Some("nonsense")), InsetPolicy::Zones);
    }

    #[test]
    fn margins_follow_the_placement_in_logical_pixels() {
        let m = wayland_metrics(InsetPolicy::Zones, 1.0, (1920, 1080), bar());
        let x = m.x_for(344);
        assert_eq!(margins(Scale::Integer(1), &m, x, m.top, (344, 100)), (8, 12));
        assert_eq!(margins(Scale::Integer(1), &m, x, m.top + 110, (344, 100)), (118, 12), "the next banner sits a gap lower");
        let m2 = wayland_metrics(InsetPolicy::Zones, 2.0, (3840, 2160), bar());
        assert_eq!(margins(Scale::Integer(2), &m2, m2.x_for(688), m2.top, (688, 200)), (8, 12));
    }

    #[test]
    fn input_regions_cover_the_device_rectangles_in_logical_pixels() {
        assert_eq!(logical_rects(&[(0, 0, 10, 4)], Scale::Integer(1)), vec![(0, 0, 10, 4)]);
        assert_eq!(logical_rects(&[(1, 1, 10, 4)], Scale::Integer(2)), vec![(0, 0, 6, 3)]);
        // at 1.5x a 3 px device rectangle at x=1 spans logical pixels 0..3
        let r = logical_rects(&[(1, 0, 4, 3)], Scale::Fractional(180));
        assert_eq!(r, vec![(0, 0, 4, 2)]);
        assert!(logical_rects(&[(0, 0, 0, 0)], Scale::Integer(2))[0].2 >= 1, "a region is never empty-sized");
        assert!(logical_rects(&[], Scale::Integer(1)).is_empty());
    }

    #[test]
    fn a_transparent_catcher_is_one_pixel_with_a_viewport_or_the_gcd_trick_without() {
        assert_eq!(transparent_plan(1920, 1080, true), (1, 1, 1));
        assert_eq!(transparent_plan(1920, 1080, false), (16, 9, 120));
        assert_eq!(transparent_plan(1000, 777, false), (1000, 777, 1));
        let (w, h, g) = transparent_plan(1366, 768, false);
        assert_eq!((w * g as u32, h * g as u32), (1366, 768));
        assert_eq!(transparent_plan(0, 0, false), (1, 1, 1));
    }

    #[test]
    fn buttons_map_to_the_stack_buttons() {
        assert_eq!(button_of(0x110), Some(Btn::Left));
        assert_eq!(button_of(0x111), Some(Btn::Right));
        assert_eq!(button_of(0x112), Some(Btn::Middle));
        assert_eq!(button_of(0x113), None);
    }

    #[test]
    fn wheel_steps_are_whole_discrete_notches_or_accumulated_pixels() {
        let mut s = ScrollAccum::default();
        assert_eq!(s.push(1, 15.0), 1);
        assert_eq!(s.push(-2, -30.0), -2);
        assert_eq!(s.push(0, 8.0), 0);
        assert_eq!(s.push(0, 8.0), 0);
        assert_eq!(s.push(0, 8.0), 1, "24 px of touchpad motion make a step and keep the remainder");
        assert_eq!(s.push(0, 15.0), 0);
        assert_eq!(s.push(0, 5.0), 1);
        assert_eq!(s.push(0, -45.0), -2);
        s.push(0, 10.0);
        s.reset();
        assert_eq!(s.push(0, 10.0), 0, "a reset forgets the remainder");
        s.push(0, 10.0);
        assert_eq!(s.push(1, 0.0), 1);
        assert_eq!(s.push(0, 10.0), 0, "a notch also forgets the pixel remainder");
    }

    #[test]
    fn key_repeat_starts_after_the_delay_repeats_at_the_rate_and_stops_on_release() {
        let t0 = Instant::now();
        let mut r = KeyRepeat::default();
        r.set_info(25, 300);
        r.press(30, 0xff09, t0);
        assert_eq!(r.due(t0 + Duration::from_millis(299)), None);
        assert_eq!(r.wait(t0), Some(Duration::from_millis(300)));
        assert_eq!(r.due(t0 + Duration::from_millis(300)), Some(0xff09));
        assert_eq!(r.due(t0 + Duration::from_millis(339)), None, "25/s is 40 ms apart");
        assert_eq!(r.due(t0 + Duration::from_millis(340)), Some(0xff09));
        // a stall does not replay the backlog
        assert_eq!(r.due(t0 + Duration::from_secs(10)), Some(0xff09));
        assert_eq!(r.due(t0 + Duration::from_secs(10)), None);
        // releasing a key that is not the held one does not stop it; releasing the held one does
        r.release(31);
        assert!(r.wait(t0).is_some());
        r.release(30);
        assert_eq!(r.wait(t0), None);
        assert_eq!(r.due(t0 + Duration::from_secs(20)), None);
    }

    #[test]
    fn a_newer_key_takes_over_modifiers_never_repeat_and_disabling_stops_everything() {
        let t0 = Instant::now();
        let mut r = KeyRepeat::default();
        r.set_info(10, 100);
        r.press(30, 0xff09, t0);
        r.press(111, 0xffff, t0);
        assert_eq!(r.due(t0 + Duration::from_millis(100)), Some(0xffff));
        r.press(50, 0xffe1, t0);
        assert_eq!(r.wait(t0), None, "Shift_L neither repeats nor leaves an older key repeating");
        r.press(30, 0xff09, t0);
        r.set_info(0, 100);
        assert_eq!(r.wait(t0), None, "a rate of zero turns repeat off");
        r.press(30, 0xff09, t0);
        assert_eq!(r.due(t0 + Duration::from_secs(5)), None);
        r.set_info(10, 100);
        r.press(30, 0xff09, t0);
        r.clear();
        assert_eq!(r.wait(t0), None, "losing the keyboard focus clears the held key");
    }

    #[test]
    fn the_named_output_wins_else_the_first() {
        let outs = vec![(Some("A".to_string()), 1), (Some("B".to_string()), 2), (None, 3)];
        assert_eq!(pick_output(&outs, Some("B")), Some(2));
        assert_eq!(pick_output(&outs, Some("missing")), Some(1));
        assert_eq!(pick_output(&outs, None), Some(1));
        assert_eq!(pick_output::<i32>(&[], Some("A")), None);
    }
}
