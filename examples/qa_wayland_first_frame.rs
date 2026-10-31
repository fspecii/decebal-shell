//! QA-only inert window for tests/e2e/wayland_first_frame.py. Never starts workers or accessibility.
use decebal::{
    app::Button,
    window_app::{KeyEvent, Response, Waker, WindowApp},
};
use std::{fs::OpenOptions, io::Write, path::PathBuf};
use tiny_skia::Pixmap;

struct Probe {
    marker: PathBuf,
    // Retain the sender: dropping the last sender itself wakes calloop with ChannelEvent::Closed.
    waker: Option<Waker>,
}
impl Probe {
    fn mark(&self, event: &str) {
        let mut f = OpenOptions::new().create(true).append(true).open(&self.marker).unwrap();
        writeln!(f, "{event}").unwrap();
    }
}
impl WindowApp for Probe {
    fn title(&self) -> String {
        "QA quiescent first frame".into()
    }
    fn app_id(&self) -> &'static str {
        "decebal-qa-first-frame"
    }
    fn initial_size(&self) -> (u32, u32) {
        (320, 240)
    }
    fn set_waker(&mut self, waker: Waker) {
        self.waker = Some(waker);
        self.mark("initial_roundtrips_returned");
    }
    fn set_screen(&mut self, _: u32, _: u32) {}
    fn resize(&mut self, _: u32, _: u32) {
        self.mark("resize");
    }
    fn set_rounded(&mut self, _: bool) {
        self.mark("configure_dispatched");
    }
    fn draw(&mut self, pm: &mut Pixmap) {
        pm.fill(tiny_skia::Color::from_rgba8(30, 60, 90, 255));
        self.mark("draw");
    }
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
        panic!("inert fixture must never be woken")
    }
}
fn main() {
    let marker = std::env::args_os().nth(1).expect("marker path");
    decebal::backend::wayland_window::run(Probe { marker: marker.into(), waker: None }).unwrap();
}
