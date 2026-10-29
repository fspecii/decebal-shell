//! The Live wallpapers tab. Network access and video decoding run outside the UI thread.
use super::{
    catalog::{self, Media, Page},
    Config,
};
use crate::settings::{
    model::{CtlVal, Model},
    ui::{Ctl, Item, Kind, Row, Thumb},
};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct State {
    pub path: String,
    pub commons: bool,
    pub selected: Option<u64>,
    pub preview_frames: u64,
    player: Option<super::preview::Player>,
    frame: Option<std::rc::Rc<tiny_skia::Pixmap>>,
    pub query: String,
    pub results: Vec<Media>,
    pub next: Option<u64>,
    busy: bool,
    rx: Option<Receiver<Result<Done, String>>>,
    last_search: Option<Instant>,
}
enum Done {
    Search(Page, bool),
    Preview(u64, PathBuf),
    Applied,
    Disabled,
}
fn button(id: &str, label: &str, enabled: bool) -> Ctl {
    let mut c = Ctl::new(id, Kind::Button { label: label.into(), danger: false });
    c.enabled = enabled;
    c
}
impl State {
    pub fn motion_frame(&self) -> Option<std::rc::Rc<tiny_skia::Pixmap>> {
        self.frame.clone()
    }
    pub fn preview_active(&self) -> bool {
        self.player.is_some()
    }
    pub fn thumbnail_path(&self, id: &str) -> Option<&std::path::Path> {
        let id = if id == "lw:motion" { self.selected? } else { id.strip_prefix("lw:pick:")?.parse::<u64>().ok()? };
        self.results.iter().find(|m| m.id == id)?.preview.as_deref()
    }
    fn stop_preview(&mut self) {
        self.player = None;
        self.frame = None;
        self.preview_frames = 0;
    }
}
pub fn items(m: &Model) -> Vec<Item> {
    let lw = &m.lw;
    let c = Config::load();
    let mut source = Ctl::new("lw:provider", Kind::Seg(vec![("DesktopHut".into(), "desktophut".into()), ("Commons".into(), "commons".into())], Some(usize::from(lw.commons))));
    source.enabled = !lw.busy;
    let search_card = Item::Card(vec![
        Row::new("").ctl(source),
        Row::new("Search").ctl(Ctl::new("lw:query", Kind::Text { val: lw.query.clone(), hint: "Search wallpapers, or leave empty for recent uploads".into() })),
        Row::new("").ctl(button("lw:search", if lw.commons { "Search Commons" } else { "Browse DesktopHut" }, !lw.busy)),
    ]);
    let mut out = vec![crate::settings::wallhaven::tab_card(&m.wh), Item::Header("Live wallpapers".into())];
    if let Some(media) = lw.results.iter().find(|v| Some(v.id) == lw.selected) {
        let playing = lw.player.is_some();
        out.push(Item::Card(vec![Row::new(&media.title).detail(if playing {
            "Live preview · muted · your desktop is unchanged"
        } else if lw.busy {
            "Loading video preview…"
        } else {
            "No video preview available; open Source for details"
        })]));
        out.push(Item::Preview { id: "lw:motion".into(), caption: if playing { "Playing preview".into() } else { "Preview".into() } });
        out.push(Item::Card(vec![Row::new("").ctl(button(&format!("lw:use:{}", media.id), "Use wallpaper", !lw.busy)).ctl(button(&format!("lw:source:{}", media.id), "Source", true)).ctl(button(
            "lw:close-preview",
            "Close",
            true,
        ))]));
    }
    out.push(search_card);
    out.push(Item::Header(format!("Wallpapers ({})", lw.results.len())));
    if lw.results.is_empty() {
        out.push(Item::Note { text: if lw.busy { "Finding wallpapers…" } else { "Browse recent uploads or search for a style. Select a thumbnail to watch a preview." }.into(), warn: false });
    } else {
        out.push(Item::Thumbs(lw.results.iter().map(|media| Thumb { id: format!("lw:pick:{}", media.id), caption: media.title.clone(), selected: lw.selected == Some(media.id) }).collect()));
    }
    if lw.next.is_some() {
        out.push(Item::Card(vec![Row::new("More wallpapers").ctl(button("lw:more", "Load more", !lw.busy))]));
    }
    out.push(Item::Header("Playback & local videos".into()));
    out.push(Item::Card(vec![
        Row::new("Video file").ctl(Ctl::new("lw:path", Kind::Text { val: lw.path.clone(), hint: "/path/to/video.mp4".into() })).ctl(button(
            "lw:set",
            "Use video",
            !lw.busy && !lw.path.trim().is_empty(),
        )),
        Row::new("Pause behind fullscreen apps").detail("Keep a frame; resume when the app leaves fullscreen").ctl(Ctl::new("lw:pause", Kind::Toggle(c.pause_fullscreen))),
        Row::new("Frame rate limit").ctl(Ctl::new("lw:fps", Kind::Stepper { v: c.fps as f64, min: 1., max: 60., step: 1., unit: "fps".into() })),
        Row::new("Desktop playback").detail(if c.enabled { "Animated wallpaper" } else { "Still wallpaper" }).ctl(button("lw:off", "Stop animation", c.enabled && !lw.busy)),
    ]));
    let status = crate::settings::io::json_load(&super::host::status_path());
    for e in status["errors"].as_array().into_iter().flatten().filter_map(|e| e.as_str()) {
        out.push(Item::Note { text: e.into(), warn: true });
    }
    out.push(Item::Note { text: "Previews are muted and stop when you leave this tab. Use wallpaper downloads the original video. Check the source page for terms and credits.".into(), warn: false });
    out
}

fn work(m: &mut Model, f: impl FnOnce() -> Result<Done, String> + Send + 'static) {
    if m.lw.busy {
        return;
    }
    let (tx, rx) = mpsc::channel();
    let wake = m.wake.clone();
    m.lw.busy = true;
    m.lw.rx = Some(rx);
    m.dirty = true;
    std::thread::spawn(move || {
        let _ = tx.send(f());
        wake();
    });
}
pub fn apply(m: &mut Model, id: &str, v: CtlVal) {
    match (id, v) {
        ("lw:provider", CtlVal::Str(s)) if !m.lw.busy && matches!(s.as_str(), "commons" | "desktophut") => {
            m.lw.stop_preview();
            m.lw.selected = None;
            m.lw.commons = s == "commons";
            m.lw.results.clear();
            m.lw.next = None;
            m.lw.last_search = None;
        }
        ("lw:close-preview", CtlVal::Click) => {
            m.lw.stop_preview();
            m.lw.selected = None;
        }
        (id, CtlVal::Click) if (id.starts_with("lw:pick:") || id.starts_with("lw:preview:")) && !m.lw.busy => {
            let id = id.rsplit(':').next().and_then(|s| s.parse::<u64>().ok());
            if let Some(media) = m.lw.results.iter().find(|a| Some(a.id) == id).cloned() {
                m.lw.stop_preview();
                m.lw.selected = Some(media.id);
                m.sel.reset_scroll = true;
                if !media.motion.is_empty() {
                    work(m, move || super::desktophut::preview(&media).map(|p| Done::Preview(media.id, p)));
                }
            }
        }
        ("lw:path", CtlVal::Str(s)) => m.lw.path = s,
        ("lw:query", CtlVal::Str(s)) => m.lw.query = s,
        ("lw:set", CtlVal::Click) => {
            let p = PathBuf::from(crate::settings::panes::expand_tilde(&m.lw.path));
            let paths = m.paths.clone();
            work(m, move || {
                super::activate(super::prepare(&p)?, &paths)?;
                Ok(Done::Applied)
            });
        }
        ("lw:off", CtlVal::Click) => work(m, || {
            super::disable()?;
            Ok(Done::Disabled)
        }),
        ("lw:pause", CtlVal::Bool(b)) => {
            let mut c = Config::load();
            c.pause_fullscreen = b;
            if let Err(e) = c.save() {
                m.say(e, true);
            }
        }
        ("lw:fps", CtlVal::Num(n)) => {
            let mut c = Config::load();
            c.fps = (n.round() as u32).clamp(1, 60);
            if let Err(e) = c.save() {
                m.say(e, true);
            }
        }
        ("lw:search" | "lw:more", CtlVal::Click) => {
            if m.lw.last_search.is_some_and(|t| t.elapsed() < Duration::from_secs(2)) {
                return;
            }
            let append = id == "lw:more";
            let offset = if append { m.lw.next.unwrap_or(0) } else { 0 };
            let query = m.lw.query.clone();
            let commons = m.lw.commons;
            if !m.lw.busy {
                m.lw.stop_preview();
                m.lw.selected = None;
                m.lw.last_search = Some(Instant::now());
                work(m, move || (if commons { catalog::search(&query, offset) } else { super::desktophut::search(&query, offset) }).map(|p| Done::Search(p, append)));
            }
        }
        (id, CtlVal::Click) if id.starts_with("lw:use:") => {
            let id = id.trim_start_matches("lw:use:").parse::<u64>().ok();
            if let Some(media) = m.lw.results.iter().find(|a| Some(a.id) == id).cloned() {
                m.lw.stop_preview();
                let paths = m.paths.clone();
                work(m, move || {
                    let p = if media.page.starts_with("https://www.desktophut.com/") { super::desktophut::download(&media)? } else { catalog::download(&media)? };
                    super::activate(super::prepare(&p)?, &paths)?;
                    Ok(Done::Applied)
                });
            }
        }
        (id, CtlVal::Click) if id.starts_with("lw:source:") => {
            let id = id.trim_start_matches("lw:source:").parse::<u64>().ok();
            if let Some(media) = m.lw.results.iter().find(|a| Some(a.id) == id) {
                crate::settings::io::spawn_detached("xdg-open", &[&media.page]);
            }
        }
        _ => {}
    }
    m.dirty = true;
}
pub fn pump(m: &mut Model) -> bool {
    let active = m.wallpaper_open() && m.wh.tab == crate::settings::wallhaven::Tab::Live;
    let mut redraw = false;
    if !active && m.lw.selected.is_some() {
        m.lw.stop_preview();
        m.lw.selected = None;
        redraw = true;
    }
    if let Some(player) = &mut m.lw.player {
        if let Some(bytes) = player.latest() {
            m.lw.frame = tiny_skia::Pixmap::from_vec(bytes, tiny_skia::IntSize::from_wh(super::preview::WIDTH, super::preview::HEIGHT).unwrap()).map(std::rc::Rc::new);
            m.lw.preview_frames += 1;
            redraw = true;
            m.dirty = true;
        }
        if player.ended() {
            m.lw.player = None;
            m.say("Could not play this preview. You can still open its source page.", true);
            redraw = true;
        }
    }
    let result = m.lw.rx.as_ref().and_then(|rx| match rx.try_recv() {
        Ok(r) => Some(r),
        Err(mpsc::TryRecvError::Disconnected) => Some(Err("Wallpaper worker stopped".into())),
        Err(_) => None,
    });
    let Some(result) = result else { return redraw };
    m.lw.busy = false;
    m.lw.rx = None;
    match result {
        Ok(Done::Search(p, append)) => {
            if !append {
                m.lw.results.clear();
            }
            for media in p.media {
                if !m.lw.results.iter().any(|m| m.page == media.page) {
                    m.lw.results.push(media);
                }
            }
            m.lw.next = p.next;
            if m.lw.results.is_empty() {
                m.say("No supported videos found. Try another search.", false);
            }
        }
        Ok(Done::Preview(id, path)) => {
            if active && m.lw.selected == Some(id) {
                match super::preview::Player::start(&path, m.wake.clone()) {
                    Ok(player) => m.lw.player = Some(player),
                    Err(e) => m.say(e, true),
                }
            }
        }
        Ok(Done::Applied) => {
            m.on_files_changed();
            let poster = Config::load().poster;
            crate::settings::themepane::follow_wallpaper(m, &poster.to_string_lossy());
            m.say("Live wallpaper enabled; fullscreen apps pause playback", false);
        }
        Ok(Done::Disabled) => m.say("Animation stopped; still frame kept", false),
        Err(e) => m.say(e, true),
    }
    m.dirty = true;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::model::tests_support::model_with;
    #[test]
    fn provider_switch_clears_pagination_and_keeps_playback_settings() {
        let (mut m, _) = model_with("[general]\n");
        assert!(!m.lw.commons);
        m.lw.next = Some(3);
        apply(&mut m, "lw:provider", CtlVal::Str("commons".into()));
        assert!(m.lw.commons);
        assert!(m.lw.next.is_none());
        assert!(m.lw.results.is_empty());
        m.lw.busy = true;
        apply(&mut m, "lw:provider", CtlVal::Str("desktophut".into()));
        assert!(m.lw.commons, "a running provider request cannot be switched underneath its result");
    }
}
