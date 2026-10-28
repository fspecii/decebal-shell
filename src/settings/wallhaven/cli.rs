//! `decthemectl wallpaper wallhaven ...`: the Wallhaven pane without the window.
//!
//! It reads the same filters and API key as the pane, downloads through the same validated path into the
//! Wallpaper pane's backgrounds folder and sets the wallpaper through the same code
//! ([`panes::set_wallpaper`](crate::settings::panes::set_wallpaper)), so it is saved in `config.toml`,
//! applied by rdwm and followed by "Colours from wallpaper".

use super::client::{self, Client, Progress};
use super::query::{Query, Sort};
use super::store::{self, Dirs};
use crate::settings::io::{self, Ev, Paths};
use crate::settings::model::Model;
use crate::settings::schema::Schema;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const HELP: &str = "decthemectl wallpaper wallhaven random [--screen WxH] [--no-set]
decthemectl wallpaper wallhaven search WORDS... [--screen WxH] [--list] [--no-set]
                                 Download a wallpaper from wallhaven.cc and set it (saved in rdwm's config like the
                                 Wallpaper pane does). `random` picks one at random; `search` takes the best match, or
                                 with --list only prints the matches (id, size, address). --no-set only downloads.
                                 Uses the filters you chose in System Settings > Wallpaper > Wallhaven (categories,
                                 resolution, ...; safe for work unless you enabled more there and saved an API key).
                                 Environment: WALLHAVEN_API_BASE (testing), XDG_CONFIG_HOME for the key and filters.";

struct Opts {
    words: Vec<String>,
    screen: Option<(u32, u32)>,
    list: bool,
    no_set: bool,
}

fn parse_screen(s: &str) -> Option<(u32, u32)> {
    let (w, h) = s.split_once(['x', 'X'])?;
    let (w, h) = (w.parse::<u32>().ok()?, h.parse::<u32>().ok()?);
    (w > 0 && h > 0).then_some((w, h))
}

fn parse_opts(args: &[String]) -> Result<Opts, String> {
    let mut o = Opts { words: vec![], screen: None, list: false, no_set: false };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--list" => o.list = true,
            "--no-set" => o.no_set = true,
            "--screen" => o.screen = Some(it.next().and_then(|s| parse_screen(s)).ok_or("--screen needs WxH, e.g. 3440x1440")?),
            s if s.starts_with("--") => return Err(format!("unknown option {s}")),
            _ => o.words.push(a.clone()),
        }
    }
    Ok(o)
}

/// This screen's size: `--screen`, else rdwm's first monitor, else unknown (no size filter).
fn screen_size(paths: &Paths, given: Option<(u32, u32)>) -> (u32, u32) {
    if let Some(s) = given {
        return s;
    }
    let rt = io::poll_runtime(&paths.socket);
    let wa = &rt.state["monitors"][0]["workarea"];
    (wa["w"].as_u64().unwrap_or(0) as u32, wa["h"].as_u64().unwrap_or(0) as u32)
}

/// Runs the command (`args` are what follows `wallpaper`). Returns the line to print.
pub fn run(args: &[String]) -> Result<String, String> {
    match args.first().map(String::as_str) {
        Some("wallhaven") => {}
        _ => return Err(format!("expected `wallhaven random|search`\n\n{HELP}")),
    }
    let sub = args.get(1).map(String::as_str).unwrap_or("");
    let o = parse_opts(&args[args.len().min(2)..])?;
    let (sort, words) = match sub {
        "random" => (Some(Sort::Random), String::new()),
        "search" if !o.words.is_empty() => (None, o.words.join(" ")),
        "search" => return Err("search needs some words, e.g. `search mountain lake`".into()),
        _ => return Err(format!("expected `random` or `search WORDS`\n\n{HELP}")),
    };
    let dirs = Dirs::discover();
    let paths = Paths::discover();
    let key = store::load_key(&dirs);
    let mut prefs = store::load_prefs(&dirs);
    let screen = screen_size(&paths, o.screen);
    prefs.query = if sub == "random" { prefs.query.clone() } else { words };
    if let Some(s) = sort {
        prefs.sort = s;
    } else if prefs.sort == Sort::Toplist && !prefs.query.is_empty() {
        prefs.sort = Sort::Relevance;
    }
    let q = Query::from_prefs(&prefs, key.is_some(), screen);
    let client = Client::new(client::api_base(), key, Arc::default());
    let page = client.search(&q, 1).map_err(|e| e.friendly())?;
    if page.walls.is_empty() {
        return Err("no wallpaper matched; try other words or looser filters".into());
    }
    if o.list {
        let lines: Vec<String> = page.walls.iter().map(|w| format!("{}  {}  {}", w.id, w.resolution, w.url)).collect();
        return Ok(lines.join("\n"));
    }
    let wall = &page.walls[if sub == "random" { (nanos() % page.walls.len() as u64) as usize } else { 0 }];
    let path = client.download(wall, &paths.backgrounds(), q.purity, &Progress::default()).map_err(|e| e.friendly())?;
    if o.no_set {
        return Ok(format!("downloaded {} ({})", path.display(), wall.resolution));
    }
    set_headless(&paths, &path.to_string_lossy())?;
    Ok(format!("wallpaper set: {} ({})", path.display(), wall.resolution))
}

fn nanos() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1)
}

/// The pane's own code path without a window: edit config.toml, reload rdwm, follow "Colours from wallpaper".
fn set_headless(paths: &Paths, path: &str) -> Result<(), String> {
    let queue: io::Queue = Arc::new(Mutex::new(Default::default()));
    let mut m = Model::new(paths.clone(), Schema::load(&paths.rdwmctl), queue.clone(), Arc::new(|| {}));
    crate::settings::panes::set_wallpaper(&mut m, path);
    if let Some(n) = m.notice.as_ref().filter(|n| n.error) {
        return Err(n.text.clone());
    }
    if m.rt.running {
        // the reload runs on a thread: wait for its answer before the process exits
        let t = Instant::now();
        while t.elapsed() < Duration::from_secs(4) {
            if queue.lock().map(|q| q.iter().any(|e| matches!(e, Ev::Reload(_)))).unwrap_or(false) {
                break;
            }
            std::thread::sleep(Duration::from_millis(30));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn options_parse() {
        let o = parse_opts(&a(&["mountain", "lake", "--screen", "3440x1440", "--list"])).unwrap();
        assert_eq!((o.words.join(" "), o.screen, o.list, o.no_set), ("mountain lake".into(), Some((3440, 1440)), true, false));
        assert!(parse_opts(&a(&["--screen", "huge"])).is_err());
        assert!(parse_opts(&a(&["--screen"])).is_err());
        assert!(parse_opts(&a(&["--frobnicate"])).is_err());
    }

    #[test]
    fn bad_commands_explain_themselves() {
        assert!(run(&a(&[])).unwrap_err().contains("wallhaven"));
        assert!(run(&a(&["wallhaven"])).unwrap_err().contains("random"));
        assert!(run(&a(&["wallhaven", "search"])).unwrap_err().contains("words"));
        assert!(run(&a(&["wallhaven", "nonsense"])).is_err());
    }
}
