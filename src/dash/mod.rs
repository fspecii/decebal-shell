//! `macdash`: a dashboard popup under the menu bar, ported from caelestia-shell's dashboard.
//!
//! Three tabs: **Overview** (user card, clock, month calendar, weather, resource rings, now playing), **Media**
//! (MPRIS player: cover, progress, controls, volume, player picker) and **Performance** (CPU, memory, storage,
//! network, optional NVIDIA GPU). The window is an X11 override-redirect popup with the keyboard and pointer
//! grabbed (see [`crate::backend::x11_window`]); it exists only while visible, so nothing runs when it is closed.
//!
//! Layout and key handling live in [`layout`], numbers in [`sysinfo`], the player in [`mpris`]; this file owns
//! the state and the event glue, [`draw`] the painting.

pub mod art;
pub mod charts;
mod draw;
pub mod info;
pub mod layout;
pub mod mpris;
pub mod sysinfo;

use crate::app::Button;
use crate::menubar::symbols::SymbolCache;
use crate::menubar::weather::{self, Weather};
use crate::text::Fonts;
use crate::window_app::{Action, KeyEvent, PopupAnchor, Response, Waker, WindowApp};
use layout::{Hit, KeyAction, Tab};
use mpris::{Cmd, Mpris, Player, Status};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use sysinfo::{Gpu, Stats};
use tiny_skia::Pixmap;

/// A pointer drag in progress on the Media tab.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Drag {
    Seek,
    Volume,
}

pub struct Dash {
    tab: Tab,
    alpha: bool,
    anchor: PopupAnchor,
    fonts: Option<Fonts>,
    symbols: SymbolCache,
    waker: Option<Waker>,
    visible: Arc<AtomicBool>,
    // numbers
    stats: Arc<Mutex<(u64, Stats)>>,
    stats_seen: u64,
    cur: Stats,
    gpu: Arc<Mutex<Option<Gpu>>>,
    // player
    mpris: Option<Mpris>,
    mpris_seen: u64,
    players: Vec<Player>,
    bus_up: bool,
    /// Picked by the user (a chip or Tab); wins over `auto` while that player exists.
    chosen: Option<String>,
    /// The player shown when nothing was picked: sticks to one until it stops and another one plays.
    auto: Option<String>,
    art: art::ArtState,
    // overview
    user: info::User,
    avatar: Option<Pixmap>,
    hour12: bool,
    weather: Arc<Mutex<Option<Weather>>>,
    wx: Option<Weather>,
    wx_loading: Arc<AtomicBool>,
    // interaction
    hover: Option<Hit>,
    drag: Option<Drag>,
    drag_value: Option<f32>,
    last_vol_sent: Option<Instant>,
    theme_seen: u64,
}

/// Where the card hangs by default: just under the classic menu bar; with the Caelestia pill bar just under it when it runs
/// along the top edge (`top`, `split`), else near the top edge of the screen.
fn default_top() -> i32 {
    let th = crate::theme::th();
    if th.pill_bar().is_some() {
        th.bar_insets().top + 10
    } else {
        crate::theme::BAR_HEIGHT as i32 + layout::BAR_GAP as i32
    }
}

impl Dash {
    /// `center_x`: where under the bar the card hangs (the monitor's middle when `None`).
    pub fn new(tab: Tab, center_x: Option<i32>, top: Option<i32>) -> Dash {
        let weather = weather::load_cached();
        Dash {
            tab,
            alpha: false,
            anchor: PopupAnchor { top: top.unwrap_or_else(default_top), center_x },
            fonts: Fonts::load(),
            symbols: SymbolCache::default(),
            waker: None,
            visible: Arc::new(AtomicBool::new(true)),
            stats: Arc::new(Mutex::new((0, Stats::default()))),
            stats_seen: 0,
            cur: Stats::default(),
            gpu: Arc::new(Mutex::new(None)),
            mpris: None,
            mpris_seen: 0,
            players: Vec::new(),
            bus_up: false,
            chosen: None,
            auto: None,
            art: art::ArtState::default(),
            user: info::load_user(),
            avatar: info::face_path().and_then(|p| info::load_square(&p, 128)),
            hour12: crate::config::shell_settings().clock_hour12,
            weather: Arc::new(Mutex::new(weather.clone())),
            wx: weather,
            wx_loading: Arc::new(AtomicBool::new(false)),
            hover: None,
            drag: None,
            drag_value: None,
            last_vol_sent: None,
            theme_seen: crate::theme::generation(),
        }
    }

    pub fn tab(&self) -> Tab {
        self.tab
    }

    /// The player on show.
    fn active(&self) -> Option<&Player> {
        let pick = self.chosen.as_deref().filter(|c| self.players.iter().any(|p| p.bus == *c)).or(self.auto.as_deref());
        mpris::pick_active(&self.players, pick)
    }

    /// Keeps the automatic choice steady: a new player starting does not steal the card from one that is playing.
    fn settle_auto(&mut self) {
        let keep =
            self.auto.as_deref().and_then(|a| self.players.iter().find(|p| p.bus == a)).is_some_and(|cur| cur.status == Status::Playing || !self.players.iter().any(|p| p.status == Status::Playing));
        if !keep {
            self.auto = mpris::pick_active(&self.players, None).map(|p| p.bus.clone());
        }
    }

    fn set_tab(&mut self, t: Tab) -> Response {
        if t == self.tab {
            return Response::NONE;
        }
        self.tab = t;
        self.drag = None;
        self.drag_value = None;
        if let Some(m) = &self.mpris {
            m.send(Cmd::Polling(t == Tab::Media));
        }
        Response::REDRAW
    }

    fn command(&self, make: impl FnOnce(String) -> Cmd) {
        if let (Some(m), Some(p)) = (&self.mpris, self.active()) {
            if p.can_control {
                m.send(make(p.bus.clone()));
            }
        }
    }

    fn adjust_volume(&mut self, delta: i32) -> Response {
        let Some(p) = self.active() else { return Response::NONE };
        let Some(v) = p.volume else { return Response::NONE };
        let bus = p.bus.clone();
        let nv = (v + f64::from(delta) / 100.0).clamp(0.0, 1.0);
        if let Some(m) = &self.mpris {
            m.send(Cmd::Volume(bus, nv));
        }
        Response::REDRAW
    }

    /// Reads everything the worker threads left for us. Returns true when something visible changed.
    fn refresh(&mut self) -> bool {
        let mut redraw = false;
        let g = crate::theme::generation();
        if g != self.theme_seen {
            self.theme_seen = g;
            redraw = true;
        }
        let mut stats_changed = false;
        if let Ok(s) = self.stats.lock() {
            if s.0 != self.stats_seen {
                self.stats_seen = s.0;
                let gpu = self.cur.gpu.take();
                self.cur = s.1.clone();
                self.cur.gpu = gpu;
                stats_changed = true;
            }
        }
        if let Ok(g) = self.gpu.lock() {
            if *g != self.cur.gpu {
                self.cur.gpu = g.clone();
                stats_changed = true;
            }
        }
        if let Some(m) = &self.mpris {
            let snap = m.snapshot();
            if snap.generation != self.mpris_seen {
                self.mpris_seen = snap.generation;
                self.players = snap.players;
                self.bus_up = snap.connected;
                self.settle_auto();
                redraw = true;
            }
        }
        let url = self.active().and_then(|p| p.meta.art_url.clone());
        if let Some(w) = self.waker.clone() {
            redraw |= self.art.want(url.as_deref(), &w);
        }
        redraw |= self.art.poll();
        if let Ok(w) = self.weather.lock() {
            if *w != self.wx {
                self.wx = w.clone();
                redraw = true;
            }
        }
        // The clock, rings and graphs move once a second; the Media tab only while something plays.
        let playing = self.active().is_some_and(|p| p.status == Status::Playing);
        redraw |= stats_changed && (self.tab != Tab::Media || playing);
        redraw
    }

    fn start_workers(&mut self, waker: &Waker) {
        // Numbers: one sample now (primes the counters), a second a moment later, then every second; paused while hidden.
        {
            let (stats, visible, wake) = (self.stats.clone(), self.visible.clone(), waker.clone());
            std::thread::spawn(move || {
                let mut sampler = sysinfo::Sampler::new(sysinfo::Paths::default());
                let mut gen = 0u64;
                let mut nap = Duration::from_millis(250);
                loop {
                    let st = sampler.sample(Instant::now());
                    gen += 1;
                    if let Ok(mut s) = stats.lock() {
                        *s = (gen, st);
                    }
                    wake();
                    std::thread::sleep(nap);
                    nap = Duration::from_secs(1);
                    while !visible.load(Ordering::Relaxed) {
                        std::thread::sleep(Duration::from_millis(250));
                    }
                }
            });
        }
        if crate::controlcenter::sys::have("nvidia-smi") {
            let (slot, visible, wake) = (self.gpu.clone(), self.visible.clone(), waker.clone());
            std::thread::spawn(move || {
                let mut fails = 0;
                loop {
                    match sysinfo::probe_gpu() {
                        Some(g) => {
                            fails = 0;
                            if let Ok(mut s) = slot.lock() {
                                *s = Some(g);
                            }
                        }
                        None => {
                            fails += 1;
                            if fails >= 3 {
                                // No usable GPU after all: hide the card for good.
                                if let Ok(mut s) = slot.lock() {
                                    *s = None;
                                }
                                wake();
                                return;
                            }
                        }
                    }
                    wake();
                    std::thread::sleep(Duration::from_secs(1));
                    while !visible.load(Ordering::Relaxed) {
                        std::thread::sleep(Duration::from_millis(250));
                    }
                }
            });
        }
        // Weather: the bar's cache when fresh, else one fetch (which refreshes the cache for the bar too).
        if weather::fresh_cached().is_none() && !self.wx_loading.swap(true, Ordering::SeqCst) {
            let (slot, loading, wake) = (self.weather.clone(), self.wx_loading.clone(), waker.clone());
            std::thread::spawn(move || {
                if let Some(w) = weather::fetch() {
                    if let Ok(mut s) = slot.lock() {
                        *s = Some(w);
                    }
                }
                loading.store(false, Ordering::SeqCst);
                wake();
            });
        } else if let Some(w) = weather::fresh_cached() {
            if let Ok(mut s) = self.weather.lock() {
                *s = Some(w);
            }
        }
        let m = Mpris::start(waker.clone());
        m.send(Cmd::Polling(self.tab == Tab::Media));
        self.mpris = Some(m);
    }
}

impl WindowApp for Dash {
    fn title(&self) -> String {
        "macdash".into()
    }

    fn app_id(&self) -> &'static str {
        "macdash"
    }

    fn overlay(&self) -> bool {
        true
    }

    fn popup_anchor(&self) -> Option<PopupAnchor> {
        Some(self.anchor)
    }

    fn popup_translucent(&self) -> bool {
        true
    }

    fn set_popup_alpha(&mut self, alpha: bool) {
        self.alpha = alpha;
    }

    fn input_grabbed(&mut self, mods: Option<u16>) -> Response {
        // Without the keyboard Escape could never close it (the pointer grab still lets a click outside do so).
        if mods.is_none() {
            eprintln!("macdash: could not grab the keyboard; click outside to close");
        }
        Response::NONE
    }

    fn initial_size(&self) -> (u32, u32) {
        (layout::CARD_W as u32, layout::CARD_H as u32)
    }

    fn content_size(&self) -> Option<(u32, u32)> {
        Some((layout::CARD_W as u32, layout::CARD_H as u32))
    }

    fn set_waker(&mut self, waker: Waker) {
        let w = waker.clone();
        crate::theme::live::start_watcher(move || w());
        self.start_workers(&waker);
        self.waker = Some(waker);
    }

    fn set_screen(&mut self, _: u32, _: u32) {}

    fn resize(&mut self, _: u32, _: u32) {}

    fn set_rounded(&mut self, _: bool) {}

    fn set_visible(&mut self, visible: bool) {
        self.visible.store(visible, Ordering::Relaxed);
    }

    fn draw(&mut self, pm: &mut Pixmap) {
        self.paint(pm);
    }

    fn pointer_motion(&mut self, x: f32, y: f32) -> Response {
        let media = self.media_layout();
        if let Some(d) = self.drag {
            match d {
                Drag::Seek => self.drag_value = Some(layout::seek_fraction(&media, x)),
                Drag::Volume => {
                    let v = layout::volume_fraction(&media, x);
                    self.drag_value = Some(v);
                    // Players are not built for a flood of Set calls: at most about 20 a second.
                    if self.last_vol_sent.is_none_or(|t| t.elapsed() > Duration::from_millis(50)) {
                        self.last_vol_sent = Some(Instant::now());
                        if let (Some(m), Some(p)) = (&self.mpris, self.active()) {
                            m.send(Cmd::Volume(p.bus.clone(), f64::from(v)));
                        }
                    }
                }
            }
            return Response::REDRAW;
        }
        let hit = layout::hit_test(self.tab, x, y, &media, self.active().is_some()).map(normalised);
        if hit != self.hover {
            self.hover = hit;
            return Response::REDRAW;
        }
        Response::NONE
    }

    fn pointer_leave(&mut self) -> Response {
        if self.hover.take().is_some() {
            Response::REDRAW
        } else {
            Response::NONE
        }
    }

    fn pointer_button(&mut self, x: f32, y: f32, button: Button, pressed: bool) -> Response {
        if button != Button::Left {
            return Response::NONE;
        }
        let media = self.media_layout();
        if !pressed {
            // Releasing a seek drag commits it.
            if let (Some(Drag::Seek), Some(f)) = (self.drag.take(), self.drag_value.take()) {
                if let Some(p) = self.active() {
                    if p.can_seek {
                        if let Some(len) = p.meta.length_us {
                            let (bus, pos) = (p.bus.clone(), (f64::from(f) * len as f64) as i64);
                            if let Some(m) = &self.mpris {
                                m.send(Cmd::Seek(bus, pos));
                            }
                        }
                    }
                }
                return Response::REDRAW;
            }
            self.drag_value = None;
            return Response::NONE;
        }
        match layout::hit_test(self.tab, x, y, &media, self.active().is_some()) {
            Some(Hit::Tab(t)) => self.set_tab(t),
            Some(Hit::Prev) => {
                if self.active().is_some_and(|p| p.can_prev) {
                    self.command(Cmd::Previous);
                }
                Response::NONE
            }
            Some(Hit::PlayPause) => {
                self.command(Cmd::PlayPause);
                Response::NONE
            }
            Some(Hit::Next) => {
                if self.active().is_some_and(|p| p.can_next) {
                    self.command(Cmd::Next);
                }
                Response::NONE
            }
            Some(Hit::Seek(f)) => {
                if self.active().is_some_and(|p| p.can_seek && p.meta.length_us.is_some()) {
                    self.drag = Some(Drag::Seek);
                    self.drag_value = Some(f);
                    Response::REDRAW
                } else {
                    Response::NONE
                }
            }
            Some(Hit::Volume(f)) => {
                self.drag = Some(Drag::Volume);
                self.drag_value = Some(f);
                self.last_vol_sent = Some(Instant::now());
                if let (Some(m), Some(p)) = (&self.mpris, self.active()) {
                    m.send(Cmd::Volume(p.bus.clone(), f64::from(f)));
                }
                Response::REDRAW
            }
            Some(Hit::Chip(i)) => {
                if let Some(p) = self.players.get(i) {
                    self.chosen = Some(p.bus.clone());
                }
                Response::REDRAW
            }
            Some(Hit::NowPlaying) => self.set_tab(Tab::Media),
            None => Response::NONE,
        }
    }

    fn scroll(&mut self, _: f32, _: f32, dy: f32) -> Response {
        if self.tab == Tab::Media {
            self.adjust_volume(if dy > 0.0 { -5 } else { 5 })
        } else {
            Response::NONE
        }
    }

    fn key(&mut self, ev: &KeyEvent) -> Response {
        match layout::key_action(self.tab, ev) {
            KeyAction::None => Response::NONE,
            KeyAction::Close => Response::act(Action::Close),
            KeyAction::Tab(t) => self.set_tab(t),
            KeyAction::PlayPause => {
                self.command(Cmd::PlayPause);
                Response::NONE
            }
            KeyAction::Volume(d) => self.adjust_volume(d),
            KeyAction::NextPlayer => {
                if self.players.len() > 1 {
                    let cur = self.active().map(|p| p.bus.clone());
                    let i = self.players.iter().position(|p| Some(&p.bus) == cur.as_ref()).unwrap_or(0);
                    self.chosen = Some(self.players[(i + 1) % self.players.len()].bus.clone());
                    return Response::REDRAW;
                }
                Response::NONE
            }
        }
    }

    fn tick(&mut self) -> Response {
        if self.refresh() {
            Response::REDRAW
        } else {
            Response::NONE
        }
    }
}

/// Hover compares targets, not where along a slider the pointer is.
fn normalised(h: Hit) -> Hit {
    match h {
        Hit::Seek(_) => Hit::Seek(0.0),
        Hit::Volume(_) => Hit::Volume(0.0),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window_app::Key;

    fn key(k: Key, ch: Option<char>) -> KeyEvent {
        KeyEvent { key: k, text: String::new(), ch, ctrl: false, shift: false, alt: false }
    }

    fn dash() -> Dash {
        Dash::new(Tab::Overview, Some(900), None)
    }

    fn player(bus: &str, status: Status) -> Player {
        let mut p = Player::new(bus, Instant::now());
        p.ready = true;
        p.status = status;
        p.meta.title = format!("title of {bus}");
        p
    }

    #[test]
    fn keys_move_between_tabs_and_close() {
        let mut d = dash();
        assert_eq!(d.tab(), Tab::Overview);
        assert!(d.key(&key(Key::Right, None)).redraw);
        assert_eq!(d.tab(), Tab::Media);
        d.key(&key(Key::Right, None));
        assert_eq!(d.tab(), Tab::Performance);
        d.key(&key(Key::Right, None));
        assert_eq!(d.tab(), Tab::Overview, "wraps");
        d.key(&key(Key::Left, None));
        assert_eq!(d.tab(), Tab::Performance);
        for (c, t) in [('2', Tab::Media), ('1', Tab::Overview), ('3', Tab::Performance)] {
            d.key(&key(Key::Char, Some(c)));
            assert_eq!(d.tab(), t);
        }
        assert_eq!(d.key(&key(Key::Char, Some('3'))), Response::NONE, "already there: nothing to redraw");
        assert_eq!(d.key(&key(Key::Escape, None)).action, Some(Action::Close));
    }

    #[test]
    fn clicking_a_tab_switches() {
        let mut d = dash();
        let r = layout::tab_rect(2);
        assert!(d.pointer_button(r.cx(), r.cy(), Button::Left, true).redraw);
        assert_eq!(d.tab(), Tab::Performance);
        let r = layout::tab_rect(1);
        assert_eq!(d.pointer_button(r.cx(), r.cy(), Button::Right, true), Response::NONE, "only the left button");
        assert_eq!(d.tab(), Tab::Performance);
        d.pointer_button(r.cx(), r.cy(), Button::Left, true);
        assert_eq!(d.tab(), Tab::Media);
        assert!(d.pointer_motion(layout::tab_rect(0).cx(), layout::tab_rect(0).cy()).redraw, "hovering a tab repaints");
        assert_eq!(d.pointer_motion(layout::tab_rect(0).cx() + 1.0, layout::tab_rect(0).cy()), Response::NONE, "same target");
        assert!(d.pointer_leave().redraw);
        assert_eq!(d.pointer_leave(), Response::NONE);
    }

    #[test]
    fn player_picking_follows_the_chip_and_tab_key() {
        let mut d = dash();
        d.players = vec![player("org.mpris.MediaPlayer2.a", Status::Paused), player("org.mpris.MediaPlayer2.b", Status::Playing)];
        d.settle_auto();
        assert_eq!(d.active().unwrap().bus, "org.mpris.MediaPlayer2.b", "the playing one by default");
        d.set_tab(Tab::Media);
        let l = d.media_layout();
        assert_eq!(l.chips.len(), 2);
        d.pointer_button(l.chips[0].cx(), l.chips[0].cy(), Button::Left, true);
        assert_eq!(d.active().unwrap().bus, "org.mpris.MediaPlayer2.a");
        d.key(&key(Key::Tab, None));
        assert_eq!(d.active().unwrap().bus, "org.mpris.MediaPlayer2.b", "Tab cycles the players on the Media tab");
        d.key(&key(Key::Tab, None));
        assert_eq!(d.active().unwrap().bus, "org.mpris.MediaPlayer2.a");
    }

    #[test]
    fn the_automatic_player_does_not_flip_when_another_starts() {
        let mut d = dash();
        d.players = vec![player("org.mpris.MediaPlayer2.z", Status::Playing)];
        d.settle_auto();
        // A second player that sorts first and also plays must not take over.
        d.players.insert(0, player("org.mpris.MediaPlayer2.a", Status::Playing));
        d.settle_auto();
        assert_eq!(d.active().unwrap().bus, "org.mpris.MediaPlayer2.z");
        // Once the shown one pauses and the other plays, follow it.
        d.players[1].status = Status::Paused;
        d.settle_auto();
        assert_eq!(d.active().unwrap().bus, "org.mpris.MediaPlayer2.a");
        // When it vanishes the choice falls back to what is left.
        d.players.remove(0);
        d.settle_auto();
        assert_eq!(d.active().unwrap().bus, "org.mpris.MediaPlayer2.z");
        // An explicit pick wins, and survives only while that player exists.
        d.players.push(player("org.mpris.MediaPlayer2.q", Status::Playing));
        d.chosen = Some("org.mpris.MediaPlayer2.q".into());
        assert_eq!(d.active().unwrap().bus, "org.mpris.MediaPlayer2.q");
        d.players.pop();
        assert_eq!(d.active().unwrap().bus, "org.mpris.MediaPlayer2.z");
    }

    #[test]
    fn the_now_playing_card_opens_the_media_tab() {
        let mut d = dash();
        let r = layout::overview_layout().playing;
        assert_eq!(d.pointer_button(r.cx(), r.cy(), Button::Left, true), Response::NONE, "no player: nothing to open");
        d.players = vec![player("org.mpris.MediaPlayer2.a", Status::Playing)];
        assert!(d.pointer_button(r.cx(), r.cy(), Button::Left, true).redraw);
        assert_eq!(d.tab(), Tab::Media);
    }

    #[test]
    fn volume_keys_and_wheel_only_act_with_a_volume_property() {
        let mut d = dash();
        d.players = vec![player("org.mpris.MediaPlayer2.a", Status::Playing)];
        d.set_tab(Tab::Media);
        assert_eq!(d.key(&key(Key::Up, None)), Response::NONE, "no Volume property: nothing to change");
        d.players[0].volume = Some(0.5);
        assert!(d.key(&key(Key::Up, None)).redraw);
        assert!(d.scroll(0.0, 0.0, 100.0).redraw);
        d.set_tab(Tab::Performance);
        assert_eq!(d.scroll(0.0, 0.0, 100.0), Response::NONE);
    }

    #[test]
    fn anchor_defaults_under_the_bar() {
        let d = Dash::new(Tab::Media, Some(640), None);
        let a = d.popup_anchor().unwrap();
        assert_eq!(a.center_x, Some(640));
        // below the classic bar, or below the pill bar's top edge (nothing there for a side or bottom bar)
        let th = crate::theme::th();
        assert!(a.top >= if th.pill_bar().is_some() { th.bar_insets().top } else { crate::theme::BAR_HEIGHT as i32 }, "below the bar: {}", a.top);
        assert_eq!(d.tab(), Tab::Media);
        assert_eq!(Dash::new(Tab::Overview, None, Some(40)).popup_anchor().unwrap().top, 40);
        assert_eq!(d.content_size(), Some((layout::CARD_W as u32, layout::CARD_H as u32)));
    }

    #[test]
    fn draws_every_tab_without_panicking() {
        let mut d = dash();
        d.players = vec![player("org.mpris.MediaPlayer2.a", Status::Playing), player("org.mpris.MediaPlayer2.b", Status::Paused)];
        d.players[0].volume = Some(0.4);
        d.players[0].meta.length_us = Some(200_000_000);
        d.cur.cores = vec![0.1, 0.9, 0.5, 0.0];
        d.cur.cpu_hist = vec![0.1, 0.5, 0.3];
        d.cur.gpu = Some(Gpu { name: "Test GPU".into(), util: Some(30.0), temp: Some(50.0), mem_used_mib: Some(100), mem_total_mib: Some(1000) });
        for t in Tab::ALL {
            d.set_tab(t);
            for alpha in [false, true] {
                d.alpha = alpha;
                let mut pm = Pixmap::new(layout::CARD_W as u32, layout::CARD_H as u32).unwrap();
                d.draw(&mut pm);
                let painted = pm.data().chunks_exact(4).filter(|p| p[3] > 0).count();
                assert!(painted > 50_000, "{t:?} painted {painted} pixels");
            }
        }
        d.players.clear();
        d.cur.gpu = None;
        for t in Tab::ALL {
            d.set_tab(t);
            let mut pm = Pixmap::new(layout::CARD_W as u32, layout::CARD_H as u32).unwrap();
            d.draw(&mut pm);
        }
    }
}
