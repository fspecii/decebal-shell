//! Geometry, tabs, hit testing and key handling of the dashboard: all pure, all unit-tested.
//! Coordinates are card pixels (the card is the whole popup window).

use crate::window_app::{Key, KeyEvent};

pub const CARD_W: f32 = 520.0;
pub const CARD_H: f32 = 392.0;
pub const PAD: f32 = 20.0;
pub const TABS_H: f32 = 52.0;
/// Gap between the bar and the card.
pub const BAR_GAP: f32 = 4.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tab {
    Overview,
    Media,
    Performance,
}

impl Tab {
    pub const ALL: [Tab; 3] = [Tab::Overview, Tab::Media, Tab::Performance];

    pub fn label(self) -> &'static str {
        match self {
            Tab::Overview => "Overview",
            Tab::Media => "Media",
            Tab::Performance => "Performance",
        }
    }

    pub fn index(self) -> usize {
        Tab::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }

    /// The tab `delta` places along, wrapping at both ends.
    pub fn step(self, delta: i32) -> Tab {
        Tab::ALL[(self.index() as i32 + delta).rem_euclid(Tab::ALL.len() as i32) as usize]
    }

    /// `1`..`3`, or a name (`overview`, `media`, `performance`, `perf`) on the command line.
    pub fn parse(s: &str) -> Option<Tab> {
        match s.to_ascii_lowercase().as_str() {
            "1" | "overview" | "dash" | "home" => Some(Tab::Overview),
            "2" | "media" | "music" | "player" => Some(Tab::Media),
            "3" | "performance" | "perf" | "system" | "stats" => Some(Tab::Performance),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
    pub fn cx(&self) -> f32 {
        self.x + self.w / 2.0
    }
    pub fn cy(&self) -> f32 {
        self.y + self.h / 2.0
    }
    pub fn inflate(&self, d: f32) -> Rect {
        Rect::new(self.x - d, self.y - d, self.w + 2.0 * d, self.h + 2.0 * d)
    }
    pub fn intersects(&self, o: &Rect) -> bool {
        self.x < o.right() && o.x < self.right() && self.y < o.bottom() && o.y < self.bottom()
    }
    /// `o` lies completely inside `self`.
    pub fn holds(&self, o: &Rect) -> bool {
        o.x >= self.x - 0.01 && o.y >= self.y - 0.01 && o.right() <= self.right() + 0.01 && o.bottom() <= self.bottom() + 0.01
    }
}

pub fn card() -> Rect {
    Rect::new(0.0, 0.0, CARD_W, CARD_H)
}

/// The area under the tab bar every tab lays itself out in.
pub fn content() -> Rect {
    Rect::new(PAD, TABS_H, CARD_W - 2.0 * PAD, CARD_H - TABS_H - PAD)
}

pub const TAB_W: f32 = 116.0;
const TAB_GAP: f32 = 6.0;

pub fn tab_rect(i: usize) -> Rect {
    let total = Tab::ALL.len() as f32 * TAB_W + (Tab::ALL.len() - 1) as f32 * TAB_GAP;
    Rect::new((CARD_W - total) / 2.0 + i as f32 * (TAB_W + TAB_GAP), 12.0, TAB_W, 28.0)
}

// ---------------------------------------------------------------- overview

#[derive(Debug, Clone, PartialEq)]
pub struct OverviewLayout {
    pub avatar: Rect,
    pub name: Rect,
    pub time: Rect,
    pub date: Rect,
    pub weather: Rect,
    pub calendar: Rect,
    pub rings: [Rect; 3],
    pub playing: Rect,
    pub divider_y: f32,
}

pub fn overview_layout() -> OverviewLayout {
    let c = content();
    let left_w = 218.0;
    let cal_x = c.x + left_w + 20.0;
    let ring_cell = 84.0;
    OverviewLayout {
        avatar: Rect::new(c.x, c.y, 56.0, 56.0),
        name: Rect::new(c.x + 68.0, c.y + 2.0, left_w - 68.0, 52.0),
        time: Rect::new(c.x, c.y + 66.0, left_w, 46.0),
        date: Rect::new(c.x, c.y + 114.0, left_w, 20.0),
        weather: Rect::new(c.x, c.y + 142.0, left_w, 60.0),
        calendar: Rect::new(cal_x, c.y, c.right() - cal_x, 204.0),
        rings: [0, 1, 2].map(|i| Rect::new(c.x + i as f32 * ring_cell, c.y + 226.0, ring_cell - 8.0, 82.0)),
        playing: Rect::new(c.x + 3.0 * ring_cell + 8.0, c.y + 226.0, c.right() - (c.x + 3.0 * ring_cell + 8.0), 82.0),
        divider_y: c.y + 214.0,
    }
}

// ---------------------------------------------------------------- media

#[derive(Debug, Clone, PartialEq)]
pub struct MediaLayout {
    pub art: Rect,
    pub title: Rect,
    pub artist: Rect,
    pub album: Rect,
    /// The bar itself (4 px high) and the larger grab area around it.
    pub bar: Rect,
    pub bar_hit: Rect,
    pub time_l: Rect,
    pub time_r: Rect,
    pub prev: Rect,
    pub play: Rect,
    pub next: Rect,
    pub vol_icon: Rect,
    pub vol_bar: Rect,
    pub vol_hit: Rect,
    /// One chip per player (empty with a single player).
    pub chips: Vec<Rect>,
    /// The name of the one player, shown small when there is no picker.
    pub who: Rect,
}

pub fn media_layout(players: usize, has_volume: bool) -> MediaLayout {
    let c = content();
    let art = Rect::new(c.x, c.y + 8.0, 208.0, 208.0);
    let col_x = art.right() + 24.0;
    let col_w = c.right() - col_x;
    let bar = Rect::new(col_x, c.y + 104.0, col_w, 5.0);
    let mid = col_x + col_w / 2.0;
    let (bs, ps) = (36.0, 52.0);
    let by = c.y + 154.0;
    let chips = if players > 1 {
        let n = players.min(5);
        let gap = 8.0;
        let w = ((c.w - gap * (n as f32 - 1.0)) / n as f32).min(150.0);
        (0..n).map(|i| Rect::new(c.x + i as f32 * (w + gap), c.bottom() - 30.0, w, 28.0)).collect()
    } else {
        Vec::new()
    };
    MediaLayout {
        art,
        title: Rect::new(col_x, c.y + 6.0, col_w, 28.0),
        artist: Rect::new(col_x, c.y + 38.0, col_w, 22.0),
        album: Rect::new(col_x, c.y + 62.0, col_w, 20.0),
        bar,
        bar_hit: Rect::new(bar.x, bar.y - 8.0, bar.w, 20.0),
        time_l: Rect::new(col_x, c.y + 118.0, 60.0, 16.0),
        time_r: Rect::new(c.right() - 60.0, c.y + 118.0, 60.0, 16.0),
        prev: Rect::new(mid - ps / 2.0 - 22.0 - bs, by + (ps - bs) / 2.0, bs, bs),
        play: Rect::new(mid - ps / 2.0, by, ps, ps),
        next: Rect::new(mid + ps / 2.0 + 22.0, by + (ps - bs) / 2.0, bs, bs),
        vol_icon: Rect::new(col_x, c.y + 234.0, 20.0, 20.0),
        vol_bar: Rect::new(col_x + 30.0, c.y + 242.0, if has_volume { col_w - 30.0 } else { 0.0 }, 5.0),
        vol_hit: Rect::new(col_x + 26.0, c.y + 232.0, if has_volume { col_w - 26.0 } else { 0.0 }, 24.0),
        chips,
        who: Rect::new(c.x, c.bottom() - 24.0, c.w, 20.0),
    }
}

// ---------------------------------------------------------------- performance

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PerfCard {
    Cpu,
    Network,
    Memory,
    Storage,
    Gpu,
}

pub fn perf_layout(has_gpu: bool) -> Vec<(PerfCard, Rect)> {
    let c = content();
    let gap = 8.0;
    let w = (c.w - gap) / 2.0;
    let (ax, bx) = (c.x, c.x + w + gap);
    let cpu_h = 204.0;
    let mut v = vec![(PerfCard::Cpu, Rect::new(ax, c.y, w, cpu_h)), (PerfCard::Network, Rect::new(ax, c.y + cpu_h + gap, w, c.h - cpu_h - gap))];
    if has_gpu {
        let (mh, gh) = (96.0_f32, 112.0_f32);
        v.push((PerfCard::Memory, Rect::new(bx, c.y, w, mh)));
        v.push((PerfCard::Storage, Rect::new(bx, c.y + mh + gap, w, mh)));
        v.push((PerfCard::Gpu, Rect::new(bx, c.y + 2.0 * (mh + gap), w, gh.min(c.h - 2.0 * (mh + gap)))));
    } else {
        let h = (c.h - gap) / 2.0;
        v.push((PerfCard::Memory, Rect::new(bx, c.y, w, h)));
        v.push((PerfCard::Storage, Rect::new(bx, c.y + h + gap, w, h)));
    }
    v
}

// ---------------------------------------------------------------- hits and keys

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hit {
    Tab(Tab),
    Prev,
    PlayPause,
    Next,
    /// Fraction along the progress bar.
    Seek(f32),
    /// Fraction along the volume slider.
    Volume(f32),
    Chip(usize),
    NowPlaying,
}

fn along(r: &Rect, x: f32) -> f32 {
    if r.w <= 0.0 {
        0.0
    } else {
        ((x - r.x) / r.w).clamp(0.0, 1.0)
    }
}

/// Fraction along the progress bar for a pointer x (drags keep working outside the bar's rectangle).
pub fn seek_fraction(l: &MediaLayout, x: f32) -> f32 {
    along(&l.bar, x)
}

pub fn volume_fraction(l: &MediaLayout, x: f32) -> f32 {
    along(&l.vol_bar, x)
}

/// What is under (`x`, `y`) on `tab`. `media` is the media layout (only used on the Media tab), `playing` whether the
/// overview shows a now-playing card to click.
pub fn hit_test(tab: Tab, x: f32, y: f32, media: &MediaLayout, playing: bool) -> Option<Hit> {
    for (i, t) in Tab::ALL.iter().enumerate() {
        if tab_rect(i).contains(x, y) {
            return Some(Hit::Tab(*t));
        }
    }
    match tab {
        Tab::Overview => (playing && overview_layout().playing.contains(x, y)).then_some(Hit::NowPlaying),
        Tab::Media => {
            if media.prev.inflate(4.0).contains(x, y) {
                Some(Hit::Prev)
            } else if media.play.inflate(4.0).contains(x, y) {
                Some(Hit::PlayPause)
            } else if media.next.inflate(4.0).contains(x, y) {
                Some(Hit::Next)
            } else if media.bar_hit.contains(x, y) {
                Some(Hit::Seek(along(&media.bar, x)))
            } else if media.vol_hit.w > 0.0 && media.vol_hit.contains(x, y) {
                Some(Hit::Volume(along(&media.vol_bar, x)))
            } else {
                media.chips.iter().position(|c| c.contains(x, y)).map(Hit::Chip)
            }
        }
        Tab::Performance => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAction {
    None,
    Close,
    Tab(Tab),
    PlayPause,
    /// Percentage points.
    Volume(i32),
    NextPlayer,
}

pub fn key_action(tab: Tab, ev: &KeyEvent) -> KeyAction {
    match ev.key {
        Key::Escape => KeyAction::Close,
        Key::Left => KeyAction::Tab(tab.step(-1)),
        Key::Right => KeyAction::Tab(tab.step(1)),
        Key::Tab if tab == Tab::Media => KeyAction::NextPlayer,
        Key::Tab => KeyAction::Tab(tab.step(if ev.shift { -1 } else { 1 })),
        Key::Up if tab == Tab::Media => KeyAction::Volume(5),
        Key::Down if tab == Tab::Media => KeyAction::Volume(-5),
        Key::Enter if tab == Tab::Media => KeyAction::PlayPause,
        Key::Char if !ev.ctrl && !ev.alt => match ev.ch {
            Some('1') => KeyAction::Tab(Tab::Overview),
            Some('2') => KeyAction::Tab(Tab::Media),
            Some('3') => KeyAction::Tab(Tab::Performance),
            Some(' ') => KeyAction::PlayPause,
            Some('q') => KeyAction::Close,
            _ => KeyAction::None,
        },
        _ => KeyAction::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(k: Key, ch: Option<char>) -> KeyEvent {
        KeyEvent { key: k, text: ch.map(String::from).unwrap_or_default(), ch, ctrl: false, shift: false, alt: false }
    }

    #[test]
    fn tabs_wrap_and_parse() {
        assert_eq!(Tab::Overview.step(1), Tab::Media);
        assert_eq!(Tab::Performance.step(1), Tab::Overview);
        assert_eq!(Tab::Overview.step(-1), Tab::Performance);
        assert_eq!(Tab::Media.step(0), Tab::Media);
        assert_eq!(Tab::parse("Media"), Some(Tab::Media));
        assert_eq!(Tab::parse("3"), Some(Tab::Performance));
        assert_eq!(Tab::parse("perf"), Some(Tab::Performance));
        assert_eq!(Tab::parse("nope"), None);
        assert_eq!(Tab::ALL.map(Tab::index), [0, 1, 2]);
    }

    #[test]
    fn keys_switch_tabs_and_close() {
        assert_eq!(key_action(Tab::Overview, &key(Key::Right, None)), KeyAction::Tab(Tab::Media));
        assert_eq!(key_action(Tab::Overview, &key(Key::Left, None)), KeyAction::Tab(Tab::Performance));
        assert_eq!(key_action(Tab::Media, &key(Key::Right, None)), KeyAction::Tab(Tab::Performance));
        for (c, t) in [('1', Tab::Overview), ('2', Tab::Media), ('3', Tab::Performance)] {
            assert_eq!(key_action(Tab::Performance, &key(Key::Char, Some(c))), KeyAction::Tab(t));
        }
        assert_eq!(key_action(Tab::Media, &key(Key::Escape, None)), KeyAction::Close);
        assert_eq!(key_action(Tab::Media, &key(Key::Char, Some(' '))), KeyAction::PlayPause);
        assert_eq!(key_action(Tab::Overview, &key(Key::Char, Some(' '))), KeyAction::PlayPause, "space is a global play/pause");
        assert_eq!(key_action(Tab::Media, &key(Key::Enter, None)), KeyAction::PlayPause);
        assert_eq!(key_action(Tab::Performance, &key(Key::Enter, None)), KeyAction::None);
        assert_eq!(key_action(Tab::Overview, &key(Key::Char, Some('4'))), KeyAction::None);
        assert_eq!(key_action(Tab::Media, &key(Key::Up, None)), KeyAction::Volume(5));
        assert_eq!(key_action(Tab::Media, &key(Key::Down, None)), KeyAction::Volume(-5));
        assert_eq!(key_action(Tab::Performance, &key(Key::Down, None)), KeyAction::None, "volume keys only on the Media tab");
        assert_eq!(key_action(Tab::Media, &key(Key::Tab, None)), KeyAction::NextPlayer);
        assert_eq!(key_action(Tab::Overview, &key(Key::Tab, None)), KeyAction::Tab(Tab::Media));
        let mut ctrl = key(Key::Char, Some('2'));
        ctrl.ctrl = true;
        assert_eq!(key_action(Tab::Overview, &ctrl), KeyAction::None, "Ctrl+2 is not a tab key");
    }

    #[test]
    fn tab_rects_sit_inside_the_card_without_overlap() {
        let c = card();
        for i in 0..3 {
            assert!(c.holds(&tab_rect(i)));
            assert!(tab_rect(i).bottom() < TABS_H);
            if i > 0 {
                assert!(!tab_rect(i - 1).intersects(&tab_rect(i)));
            }
        }
        let (first, last) = (tab_rect(0), tab_rect(2));
        assert!((first.x - (CARD_W - last.right())).abs() < 0.01, "centred");
    }

    #[test]
    fn overview_pieces_fit_and_do_not_collide() {
        let l = overview_layout();
        let c = content();
        let pieces = [l.avatar, l.name, l.time, l.date, l.weather, l.calendar, l.rings[0], l.rings[1], l.rings[2], l.playing];
        for r in &pieces {
            assert!(c.holds(r), "{r:?} outside {c:?}");
        }
        for (i, a) in pieces.iter().enumerate() {
            for b in &pieces[i + 1..] {
                // The name block sits next to the avatar, never over it; the date/time blocks are stacked.
                assert!(!a.intersects(b), "{a:?} overlaps {b:?}");
            }
        }
        assert!(l.divider_y > l.weather.bottom() && l.divider_y > l.calendar.bottom() && l.divider_y < l.rings[0].y);
    }

    #[test]
    fn media_pieces_fit_and_do_not_collide() {
        for (players, vol) in [(1, true), (1, false), (3, true), (9, true)] {
            let l = media_layout(players, vol);
            let c = content();
            let mut pieces = vec![l.art, l.title, l.artist, l.album, l.bar_hit, l.time_l, l.prev, l.play, l.next, l.vol_icon];
            if vol {
                pieces.push(l.vol_hit);
            }
            pieces.extend(l.chips.iter().copied());
            for r in &pieces {
                assert!(c.holds(r), "{r:?} outside the content");
            }
            for (i, a) in pieces.iter().enumerate() {
                for b in &pieces[i + 1..] {
                    assert!(!a.intersects(b), "{a:?} overlaps {b:?} ({players} players)");
                }
            }
            assert_eq!(l.chips.len(), if players > 1 { players.min(5) } else { 0 });
            assert!(l.time_r.x >= l.time_l.right());
            if players > 1 {
                assert!(l.chips[0].y > l.art.bottom(), "chips below the art");
            }
        }
    }

    #[test]
    fn performance_cards_tile_the_content() {
        for gpu in [false, true] {
            let cards = perf_layout(gpu);
            assert_eq!(cards.len(), if gpu { 5 } else { 4 });
            let c = content();
            for (i, (_, a)) in cards.iter().enumerate() {
                assert!(c.holds(a), "{a:?}");
                for (_, b) in &cards[i + 1..] {
                    assert!(!a.intersects(b), "{a:?} overlaps {b:?}");
                }
            }
            let area: f32 = cards.iter().map(|(_, r)| r.w * r.h).sum();
            assert!(area > 0.9 * c.w * c.h, "cards cover the content ({area})");
            assert_eq!(cards.iter().any(|(k, _)| *k == PerfCard::Gpu), gpu);
        }
    }

    #[test]
    fn hit_testing() {
        let l = media_layout(2, true);
        let at = |tab, r: Rect| hit_test(tab, r.cx(), r.cy(), &l, true);
        assert_eq!(at(Tab::Media, tab_rect(2)), Some(Hit::Tab(Tab::Performance)));
        assert_eq!(at(Tab::Overview, tab_rect(1)), Some(Hit::Tab(Tab::Media)), "tabs work from every tab");
        assert_eq!(at(Tab::Media, l.play), Some(Hit::PlayPause));
        assert_eq!(at(Tab::Media, l.prev), Some(Hit::Prev));
        assert_eq!(at(Tab::Media, l.next), Some(Hit::Next));
        assert_eq!(at(Tab::Media, l.chips[1]), Some(Hit::Chip(1)));
        assert_eq!(at(Tab::Overview, l.play), None, "media controls are inert on other tabs");
        assert_eq!(at(Tab::Overview, overview_layout().playing), Some(Hit::NowPlaying));
        assert_eq!(hit_test(Tab::Overview, overview_layout().playing.cx(), overview_layout().playing.cy(), &l, false), None, "nothing to open without a player");
        match hit_test(Tab::Media, l.bar.x + l.bar.w * 0.25, l.bar.cy(), &l, true) {
            Some(Hit::Seek(f)) => assert!((f - 0.25).abs() < 0.01),
            other => panic!("{other:?}"),
        }
        match hit_test(Tab::Media, l.vol_bar.x + l.vol_bar.w * 0.9, l.vol_bar.cy(), &l, true) {
            Some(Hit::Volume(f)) => assert!((f - 0.9).abs() < 0.01),
            other => panic!("{other:?}"),
        }
        let novol = media_layout(1, false);
        assert_eq!(hit_test(Tab::Media, novol.vol_icon.cx(), novol.vol_icon.cy(), &novol, true), None);
        assert_eq!(hit_test(Tab::Performance, 100.0, 200.0, &l, true), None);
        assert_eq!(seek_fraction(&l, l.bar.x - 50.0), 0.0, "drags past the ends clamp");
        assert_eq!(seek_fraction(&l, l.bar.right() + 50.0), 1.0);
        assert_eq!(volume_fraction(&novol, 300.0), 0.0, "no volume slider");
    }
}
