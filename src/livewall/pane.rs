//! The Live wallpapers tab. Network access and video decoding run outside the UI thread.
use super::{
    catalog::{self, Media, Page},
    Config,
};
use crate::settings::{
    model::{CtlVal, Model},
    ui::{Ctl, Item, Kind, Row},
};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct State {
    pub path: String,
    pub commons: bool,
    pub query: String,
    pub results: Vec<Media>,
    pub next: Option<u64>,
    busy: bool,
    rx: Option<Receiver<Result<Done, String>>>,
    last_search: Option<Instant>,
}
enum Done {
    Search(Page, bool),
    Applied,
    Disabled,
}
fn button(id: &str, label: &str, enabled: bool) -> Ctl {
    let mut c = Ctl::new(id, Kind::Button { label: label.into(), danger: false });
    c.enabled = enabled;
    c
}
pub fn items(m: &Model) -> Vec<Item> {
    let lw = &m.lw;
    let c = Config::load();
    let mut rows = vec![
        Row::new("Video file").detail("MP4, WebM, MKV, MOV or OGV; videos are always muted").ctl(Ctl::new("lw:path", Kind::Text { val: lw.path.clone(), hint: "/path/to/video.webm".into() })),
        Row::new("Local video").ctl(button("lw:set", "Use this video", !lw.busy && !lw.path.trim().is_empty())),
        Row::new("Pause behind fullscreen apps").detail("Keep the current frame; resume when this monitor's fullscreen app leaves").ctl(Ctl::new("lw:pause", Kind::Toggle(c.pause_fullscreen))),
        Row::new("Frame rate limit").detail("Lower rates save power; default 24 fps").ctl(Ctl::new("lw:fps", Kind::Stepper { v: c.fps as f64, min: 1., max: 60., step: 1., unit: "fps".into() })),
        Row::new("Playback").detail(if c.enabled { c.video.display().to_string() } else { "Still wallpaper".into() }).ctl(button("lw:off", "Stop animation", c.enabled && !lw.busy)),
    ];
    if lw.busy {
        rows.push(Row::new("Working…").detail("Downloading or preparing wallpaper; your current selection stays until it is ready"));
    }
    let mut out = vec![crate::settings::wallhaven::tab_card(&m.wh), Item::Header("Live wallpapers".into()), Item::Card(rows)];
    if c.poster.is_file() {
        out.push(Item::Preview { id: c.poster.to_string_lossy().into_owned(), caption: "Still frame fallback".into() });
    }
    let status = crate::settings::io::json_load(&super::host::status_path());
    if let Some(errors) = status["errors"].as_array() {
        for e in errors {
            if let Some(e) = e.as_str() {
                out.push(Item::Note { text: e.into(), warn: true });
            }
        }
    }
    out.push(Item::Note {
        text: "Requires ffmpeg and mpv on X11; Wayland uses mpvpaper. Playback pauses on covered outputs. Selecting a picture or a theme wallpaper stops the video.".into(),
        warn: false,
    });
    out.push(Item::Header(if lw.commons { "Wikimedia Commons videos" } else { "DesktopHut live wallpapers" }.into()));
    out.push(Item::Card(vec![
        Row::new("Source").ctl(Ctl::new("lw:provider", Kind::Seg(vec![("DesktopHut".into(), "desktophut".into()), ("Commons".into(), "commons".into())], Some(usize::from(lw.commons))))),
        Row::new("Search").detail("Try clouds, waterfall, ocean or abstract animation").ctl(Ctl::new("lw:query", Kind::Text { val: lw.query.clone(), hint: "landscape nature".into() })),
        Row::new("Find videos")
            .detail(if lw.commons { "Openly licensed videos; no API key required" } else { "Curated animated wallpapers; leave search empty to browse recent uploads" })
            .ctl(button("lw:search", if lw.commons { "Search Commons" } else { "Search DesktopHut" }, !lw.busy)),
    ]));
    out.push(Item::Note {
        text: if lw.commons {
            "Commons is a general media library. Check each source page for content and attribution."
        } else {
            "DesktopHut downloads use the original MP4. Check source terms; wallpapers are not assumed to be openly licensed."
        }
        .to_string()
            + " Downloads keep source details beside the video.",
        warn: false,
    });
    for media in &lw.results {
        if let Some(p) = &media.preview {
            out.push(Item::Preview { id: p.to_string_lossy().into_owned(), caption: media.title.clone() });
        }
        out.push(Item::Card(vec![
            Row::new(&media.title).detail(if media.bytes == 0 {
                "MP4 · resolution and size on source page · ".to_string() + &media.license
            } else {
                format!("{} × {} · {:.1} MB · {}", media.width, media.height, media.bytes as f64 / 1048576., media.license)
            }),
            Row::new("Creator").detail(&media.artist).ctl(button(&format!("lw:source:{}", media.id), "Source and licence", true)),
            Row::new("Live wallpaper").ctl(button(&format!("lw:use:{}", media.id), "Download and use", !lw.busy)),
        ]));
    }
    if lw.next.is_some() {
        out.push(Item::Card(vec![Row::new("More results").ctl(button("lw:more", "Load more", !lw.busy))]));
    }
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
            m.lw.commons = s == "commons";
            m.lw.results.clear();
            m.lw.next = None;
            m.lw.last_search = None;
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
                m.lw.last_search = Some(Instant::now());
                work(m, move || (if commons { catalog::search(&query, offset) } else { super::desktophut::search(&query, offset) }).map(|p| Done::Search(p, append)));
            }
        }
        (id, CtlVal::Click) if id.starts_with("lw:use:") => {
            let id = id.trim_start_matches("lw:use:").parse::<u64>().ok();
            if let Some(media) = m.lw.results.iter().find(|a| Some(a.id) == id).cloned() {
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
    let result = m.lw.rx.as_ref().and_then(|rx| match rx.try_recv() {
        Ok(r) => Some(r),
        Err(mpsc::TryRecvError::Disconnected) => Some(Err("Wallpaper worker stopped".into())),
        Err(_) => None,
    });
    let Some(result) = result else { return false };
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
