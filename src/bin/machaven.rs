//! machaven: browse wallhaven.cc and set a wallpaper. Works on Wayland and X11.

use macshell::wallhaven::ui::WallhavenApp;
use macshell::window_app::run_window;

const HELP: &str = "machaven [OPTIONS] [SEARCH WORDS...]

Browse wallhaven.cc in a window; click a wallpaper to set it, right-click to just download it.

Options:
  --recent           open on the Recent tab (wallpapers you've used)
  --screen WxH       size to match with 'fits my screen' (default: detected)
  -h, --help         show this help
  -V, --version      show the version

Keys: Enter search, Up/Down/PageUp/PageDown scroll, Ctrl+Home/End jump, Esc close.
Set MACSHELL_BACKEND=x11 or wayland to force a display backend.";

fn parse_screen(s: &str) -> Option<(u32, u32)> {
    let (w, h) = s.split_once(['x', 'X', '×'])?;
    let (w, h) = (w.trim().parse::<u32>().ok()?, h.trim().parse::<u32>().ok()?);
    (w > 0 && h > 0).then_some((w, h))
}

fn main() {
    macshell::mem::limit_arenas();
    let mut words: Vec<String> = Vec::new();
    let mut recent = false;
    let mut screen = std::env::var("MACHAVEN_SCREEN").ok().and_then(|s| parse_screen(&s));
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-h" | "--help" => {
                println!("{HELP}");
                return;
            }
            "-V" | "--version" => {
                println!("machaven {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "--recent" => recent = true,
            "--screen" => match args.next().as_deref().and_then(parse_screen) {
                Some(s) => screen = Some(s),
                None => {
                    eprintln!("machaven: --screen needs WxH, e.g. 3440x1440");
                    std::process::exit(2);
                }
            },
            s if s.starts_with('-') && s.len() > 1 => {
                eprintln!("machaven: unknown option {s}\n\n{HELP}");
                std::process::exit(2);
            }
            _ => words.push(a),
        }
    }

    let app = WallhavenApp::new(&words.join(" "), recent, screen);
    if let Err(e) = run_window(app) {
        eprintln!("machaven: {e}");
        std::process::exit(1);
    }
}
