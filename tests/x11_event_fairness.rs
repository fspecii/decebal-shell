#![cfg(feature = "x11")]
use decebal::app::{App, Button, Tick};
use decebal::backend::{x11, Anchor, SurfaceSpec};
use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tiny_skia::Pixmap;
use x11rb::{connection::Connection, protocol::xproto::*};

struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
struct Probe {
    start: Instant,
    ticks: Arc<Mutex<Vec<Duration>>>,
}
impl App for Probe {
    fn size(&self) -> (u32, u32) {
        (64, 64)
    }
    fn draw(&mut self, _: &mut Pixmap) {}
    fn pointer_motion(&mut self, _: f32, _: f32) -> bool {
        true
    }
    fn pointer_leave(&mut self) -> bool {
        false
    }
    fn pointer_button(&mut self, _: f32, _: f32, _: Button) -> bool {
        false
    }
    fn quit_requested(&self) -> bool {
        self.start.elapsed() > Duration::from_secs(2)
    }
    fn tick(&mut self) -> Tick {
        self.ticks.lock().unwrap().push(self.start.elapsed());
        Tick { redraw: true, next: Some(Duration::from_millis(16)) }
    }
}

#[test]
fn continuous_pointer_input_does_not_starve_animation_ticks() {
    let mut server = Server(
        Command::new("Xvfb")
            .args(["-displayfd", "1", "-screen", "0", "640x480x24", "-nolisten", "tcp"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("Xvfb required for native X11 regression"),
    );
    let mut display = String::new();
    BufReader::new(server.0.stdout.take().unwrap()).read_line(&mut display).unwrap();
    let display = format!(":{}", display.trim());
    std::env::set_var("DISPLAY", &display);
    let ticks = Arc::new(Mutex::new(Vec::new()));
    let start = Instant::now();
    let sender = std::thread::spawn(move || {
        let (c, screen) = x11rb::connect(Some(&display)).unwrap();
        let root = c.setup().roots[screen].root;
        let win = loop {
            let children = c.query_tree(root).unwrap().reply().unwrap().children;
            if let Some(w) = children.first() {
                break *w;
            }
            std::thread::sleep(Duration::from_millis(2));
        };
        std::thread::sleep(Duration::from_millis(200));
        // Keep the queue populated, as with a high polling-rate physical mouse.
        while start.elapsed() < Duration::from_millis(1500) {
            for i in 0..256 {
                let event = MotionNotifyEvent {
                    response_type: MOTION_NOTIFY_EVENT,
                    detail: Motion::NORMAL,
                    sequence: 0,
                    time: 0,
                    root,
                    event: win,
                    child: 0,
                    root_x: i,
                    root_y: 10,
                    event_x: i,
                    event_y: 10,
                    state: KeyButMask::default(),
                    same_screen: true,
                };
                c.send_event(false, win, EventMask::POINTER_MOTION, event).unwrap();
            }
            c.flush().unwrap();
        }
    });
    x11::run(Probe { start, ticks: ticks.clone() }, SurfaceSpec { anchor: Anchor::Top, margin: 0, reserve: 0, stretch: false, namespace: "event-fairness" }).unwrap();
    sender.join().unwrap();
    let ticks = ticks.lock().unwrap();
    let active: Vec<_> = ticks.iter().copied().filter(|t| *t >= Duration::from_millis(400) && *t < Duration::from_millis(1400)).collect();
    assert!(active.len() >= 20, "animation starved during mouse input: {} ticks", active.len());
    assert!(active.windows(2).all(|p| p[1] - p[0] < Duration::from_millis(150)), "long animation pause during pointer flood");
}
