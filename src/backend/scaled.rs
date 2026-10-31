//! A panel drawn at a display scale on X11 (`Xft.dpi` or `DECEBAL_SCALE`): the app keeps working in logical pixels
//! and this wrapper turns its sizes, strut, input region and drawing into surface pixels, and pointer positions back.
//! Drawing goes through [`crate::gfx::with_render_scale`], so shapes, icons and text are drawn at the surface size
//! (text rasterised there, sharp), not scaled up afterwards. Wayland compositors scale surfaces themselves.

use crate::app::{App, Button, InputRect, Key, NavKey, Tick, Waker};
use crate::backend::SurfaceSpec;
use std::time::Duration;
use tiny_skia::Pixmap;

pub struct Scaled<A> {
    app: A,
    base: f32,
    physical_output: Option<(u32, u32)>,
    output_scale: f32,
}

impl<A: App> Scaled<A> {
    pub fn new(app: A, scale: f32) -> Scaled<A> {
        Scaled { app, base: if scale.is_finite() { scale.clamp(0.75, 4.0) } else { 1.0 }, physical_output: None, output_scale: 1.0 }
    }
    pub fn inner(&self) -> &A {
        &self.app
    }
    pub fn inner_mut(&mut self) -> &mut A {
        &mut self.app
    }
    /// The scale in force now: the display's, unless the app applies it itself (see [`App::backend_scaled`]).
    fn scale(&self) -> f32 {
        if self.app.backend_scaled() {
            self.base
        } else {
            1.0
        }
    }
    fn refresh_output(&mut self) {
        let s = self.scale();
        if let Some((w, h)) = self.physical_output {
            let floor = |v: u32| (v as f32 / s).floor() as u32;
            crate::gfx::with_render_scale(s, || self.app.output_size(floor(w), floor(h)));
        }
        self.output_scale = s;
    }
    fn refresh_changed_scale(&mut self) {
        if self.output_scale != self.scale() {
            self.refresh_output();
        }
    }
    fn up(&self, v: u32) -> u32 {
        // 0 means "as long as the output" and stays 0
        if v == 0 {
            0
        } else {
            (v as f32 * self.scale()).ceil() as u32
        }
    }
    fn down(&self, v: u32) -> u32 {
        (v as f32 / self.scale()).round() as u32
    }
    fn rect(&self, r: InputRect) -> InputRect {
        let s = self.scale();
        let (x0, y0) = ((r.x as f32 * s).floor() as i32, (r.y as f32 * s).floor() as i32);
        let (x1, y1) = (((r.x + r.w) as f32 * s).ceil() as i32, ((r.y + r.h) as f32 * s).ceil() as i32);
        InputRect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
    }
    fn with<R>(&self, f: impl FnOnce() -> R) -> R {
        crate::gfx::with_render_scale(self.scale(), f)
    }
}

impl<A: App> App for Scaled<A> {
    fn size(&self) -> (u32, u32) {
        let (w, h) = self.with(|| self.app.size());
        (self.up(w), self.up(h))
    }
    fn resized(&mut self, w: u32, h: u32) {
        let (w, h) = (self.down(w), self.down(h));
        let s = self.scale();
        crate::gfx::with_render_scale(s, || self.app.resized(w, h))
    }
    fn surface_origin(&mut self, x: i32, y: i32) {
        let s = self.scale();
        crate::gfx::with_render_scale(s, || self.app.surface_origin(x, y))
    }
    fn draw(&mut self, pm: &mut Pixmap) {
        let s = self.scale();
        crate::gfx::with_render_scale(s, || self.app.draw(pm))
    }
    fn glass_regions(&self) -> Vec<crate::glass::Region> {
        let s = self.scale();
        self.with(|| self.app.glass_regions()).into_iter().map(|r| crate::glass::Region { x: r.x * s, y: r.y * s, w: r.w * s, h: r.h * s, radius: r.radius * s }).collect()
    }
    fn pointer_motion(&mut self, x: f32, y: f32) -> bool {
        let s = self.scale();
        crate::gfx::with_render_scale(s, || self.app.pointer_motion(x / s, y / s))
    }
    fn pointer_leave(&mut self) -> bool {
        let s = self.scale();
        crate::gfx::with_render_scale(s, || self.app.pointer_leave())
    }
    fn pointer_button(&mut self, x: f32, y: f32, button: Button) -> bool {
        let s = self.scale();
        crate::gfx::with_render_scale(s, || self.app.pointer_button(x / s, y / s, button))
    }
    fn pointer_release(&mut self, x: f32, y: f32, button: Button) -> bool {
        let s = self.scale();
        crate::gfx::with_render_scale(s, || self.app.pointer_release(x / s, y / s, button))
    }
    fn scroll(&mut self, x: f32, y: f32, dy: f32) -> bool {
        let s = self.scale();
        crate::gfx::with_render_scale(s, || self.app.scroll(x / s, y / s, dy))
    }
    fn key(&mut self, key: Key) -> bool {
        let s = self.scale();
        crate::gfx::with_render_scale(s, || self.app.key(key))
    }
    fn nav_key(&mut self, key: NavKey) -> bool {
        let s = self.scale();
        crate::gfx::with_render_scale(s, || self.app.nav_key(key))
    }
    fn home_end(&mut self, end: bool) -> bool {
        let s = self.scale();
        crate::gfx::with_render_scale(s, || self.app.home_end(end))
    }
    fn backend_scaled(&self) -> bool {
        self.app.backend_scaled()
    }
    fn output_size(&mut self, w: u32, h: u32) {
        self.physical_output = Some((w, h));
        self.refresh_output();
    }
    fn wants_pointer_grab(&self) -> bool {
        self.app.wants_pointer_grab()
    }
    fn wants_keyboard(&self) -> bool {
        self.app.wants_keyboard()
    }
    fn input_rect(&self) -> Option<InputRect> {
        self.with(|| self.app.input_rect()).map(|r| self.rect(r))
    }
    fn input_rects(&self) -> Vec<InputRect> {
        self.with(|| self.app.input_rects()).into_iter().map(|r| self.rect(r)).collect()
    }
    fn surface_spec(&self) -> Option<SurfaceSpec> {
        let s = self.scale();
        self.with(|| self.app.surface_spec()).map(|sp| SurfaceSpec { margin: (sp.margin as f32 * s).round() as i32, reserve: (sp.reserve as f32 * s).ceil() as i32, ..sp })
    }
    fn quit_requested(&self) -> bool {
        self.app.quit_requested()
    }
    fn set_waker(&mut self, waker: Waker) {
        let s = self.scale();
        crate::gfx::with_render_scale(s, || self.app.set_waker(waker))
    }
    fn idle(&mut self) -> Option<Duration> {
        let s = self.scale();
        let next = crate::gfx::with_render_scale(s, || self.app.idle());
        self.refresh_changed_scale();
        next
    }
    fn tick_in(&self) -> Option<Duration> {
        self.app.tick_in()
    }
    fn tick(&mut self) -> Tick {
        let s = self.scale();
        let tick = crate::gfx::with_render_scale(s, || self.app.tick());
        self.refresh_changed_scale();
        tick
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Probe {
        size: (u32, u32),
        resized: (u32, u32),
        last: (f32, f32),
        drawn_at: f32,
    }
    impl App for Probe {
        fn size(&self) -> (u32, u32) {
            self.size
        }
        fn resized(&mut self, w: u32, h: u32) {
            self.resized = (w, h);
        }
        fn draw(&mut self, pm: &mut Pixmap) {
            self.drawn_at = crate::gfx::render_scale();
            crate::gfx::fill_rect(pm, 10.0, 10.0, 10.0, 10.0, crate::gfx::color(255, 0, 0, 1.0));
        }
        fn pointer_motion(&mut self, x: f32, y: f32) -> bool {
            self.last = (x, y);
            true
        }
        fn pointer_leave(&mut self) -> bool {
            false
        }
        fn pointer_button(&mut self, _: f32, _: f32, _: Button) -> bool {
            false
        }
        fn input_rects(&self) -> Vec<InputRect> {
            vec![InputRect { x: 0, y: 0, w: 100, h: 26 }]
        }
        fn surface_spec(&self) -> Option<SurfaceSpec> {
            Some(SurfaceSpec { anchor: crate::backend::Anchor::Top, margin: 13, reserve: 26, stretch: true, namespace: "t" })
        }
    }

    #[test]
    fn sizes_struts_input_and_pointer_cross_the_scale_and_drawing_happens_at_it() {
        for s in [1.0, 1.25, 2.0] {
            let mut a = Scaled::new(Probe { size: (0, 420), resized: (0, 0), last: (0.0, 0.0), drawn_at: 0.0 }, s);
            assert_eq!(a.size(), (0, (420.0 * s).ceil() as u32), "0 stays 'as long as the output'");
            let spec = a.surface_spec().unwrap();
            assert_eq!((spec.reserve, spec.margin), ((26.0 * s).ceil() as i32, (13.0 * s).round() as i32));
            assert_eq!(a.input_rects()[0], InputRect { x: 0, y: 0, w: (100.0 * s).ceil() as i32, h: (26.0 * s).ceil() as i32 });
            a.resized((1600.0 * s) as u32, (420.0 * s) as u32);
            assert_eq!(a.inner().resized, (1600, 420));
            a.pointer_motion(50.0 * s, 20.0 * s);
            assert_eq!(a.inner().last, (50.0, 20.0));
            let mut pm = Pixmap::new((40.0 * s) as u32, (40.0 * s) as u32).unwrap();
            a.draw(&mut pm);
            assert_eq!(a.inner().drawn_at, s);
            // the 10..20 logical square covers 10s..20s surface pixels
            let at = |x: u32, y: u32| pm.pixel(x, y).unwrap().alpha();
            assert_eq!(at((15.0 * s) as u32, (15.0 * s) as u32), 255);
            assert_eq!(at((22.0 * s) as u32, (15.0 * s) as u32), 0);
            assert_eq!(crate::gfx::render_scale(), 1.0, "the scale ends with the call");
        }
    }

    struct SelfScaling {
        scaled: bool,
        output: (u32, u32),
    }
    impl App for SelfScaling {
        fn size(&self) -> (u32, u32) {
            (100, 40)
        }
        fn draw(&mut self, _: &mut Pixmap) {}
        fn pointer_motion(&mut self, _: f32, _: f32) -> bool {
            false
        }
        fn pointer_leave(&mut self) -> bool {
            false
        }
        fn pointer_button(&mut self, _: f32, _: f32, _: Button) -> bool {
            false
        }
        fn backend_scaled(&self) -> bool {
            self.scaled
        }
        fn output_size(&mut self, w: u32, h: u32) {
            self.output = (w, h);
        }
    }

    #[test]
    fn an_app_that_scales_itself_is_left_alone_and_the_output_arrives_in_logical_pixels() {
        let mut a = Scaled::new(SelfScaling { scaled: true, output: (0, 0) }, 2.0);
        assert_eq!(a.size(), (200, 80));
        a.output_size(1367, 601);
        assert_eq!(a.inner().output, (683, 300), "floored, so the app never believes the output is larger than it is");
        a.inner_mut().scaled = false;
        assert_eq!(a.size(), (100, 40), "an app that applies the scale itself keeps its own size");
        a.output_size(1366, 600);
        assert_eq!(a.inner().output, (1366, 600));
    }
}
