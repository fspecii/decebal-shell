//! decsettings: System Settings for rdwm and the decebal panels. Works on Wayland and X11.

use decebal::settings::app::SettingsApp;
use decebal::settings::io::Paths;
use decebal::window_app::run_window;

const HELP: &str = "decsettings [OPTIONS] [PANE]

Settings for the rdwm window manager and the decebal dock and menu bar. Edits ~/.config/rdwm/config.toml
in place (comments and unknown keys are kept), applies changes live, and follows edits made by hand.

PANE is one of: welcome (Getting Started; aliases: help, getting-started) general appearance wallpaper widgets tiling windows rules keyboard autostart displays config (and the system panes: sound network
bluetooth notifications inputdev power defaults datetime users privacy storage accessibility). `appearance` is the home of how the
desktop looks (tabs: Themes, Palettes, Customise, All apps). These older names open their tab: theme, icons, desktop; `wallpaper` is a separate sidebar section;
also appearance/fonts, appearance/window, appearance/dock, appearance/palettes, appearance/colours, appearance/apps. config = Config & Presets (edit the
files as text, save/apply/export/import presets, see docs/presets.md). `widgets` manages desktop cards across all themes.

Options:
  --config PATH   rdwm config file to edit (default: $DECSETTINGS_CONFIG, $XDG_CONFIG_HOME/rdwm/config.toml)
  -h, --help      show this help
  -V, --version   show the version

Keys: Tab/Shift+Tab move, arrows adjust, Space/Enter activate, Ctrl+F search, Esc or Ctrl+W close.
Environment: DECSETTINGS_SCALE (UI scale), DECSETTINGS_CONFIG, DECEBAL_CONFIG (folder with dock-settings.json),
RDWMCTL (path of rdwmctl), RDWM_SOCK.";

fn main() {
    decebal::compat::init();
    decebal::mem::limit_arenas();
    let mut pane: Option<String> = None;
    let mut paths = Paths::discover();
    let mut explicit_config = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-h" | "--help" => {
                println!("{HELP}");
                return;
            }
            "-V" | "--version" => {
                println!("decsettings {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "--config" => match args.next() {
                Some(p) => {
                    paths.rdwm_config = p.into();
                    explicit_config = true;
                }
                None => {
                    eprintln!("decsettings: --config needs a path");
                    std::process::exit(2);
                }
            },
            s if s.starts_with('-') => {
                eprintln!("decsettings: unknown option {s}\n\n{HELP}");
                std::process::exit(2);
            }
            _ => {
                if pane.is_some() {
                    eprintln!("decsettings: only one PANE may be supplied");
                    std::process::exit(2);
                }
                pane = Some(a);
            }
        }
    }
    if let Some(name) = pane.as_deref() {
        pane = Some(decebal::settings::navigation::resolve(name).unwrap_or_else(|| {
            eprintln!("decsettings: unknown pane {name}\n\n{HELP}");
            std::process::exit(2);
        }));
    }
    // Reopening Settings always reveals/reuses the current window; never toggle it off.
    let guard = decebal::single::hold("decsettings");
    let endpoint = decebal::settings::navigation::endpoint().unwrap_or_else(|e| {
        eprintln!("decsettings: navigation endpoint: {e}");
        std::process::exit(1);
    });
    let Some(_guard) = guard else {
        // The lock is acquired before the listener is bound; tolerate concurrent startup.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let error = loop {
            let result = match pane.as_deref() {
                Some(pane) if explicit_config => decebal::settings::navigation::send_for_config(&endpoint, pane, &paths.rdwm_config),
                Some(pane) => decebal::settings::navigation::send(&endpoint, pane),
                None => decebal::settings::navigation::activate(&endpoint, explicit_config.then_some(paths.rdwm_config.as_path())),
            };
            match result {
                Ok(()) => return,
                Err(e) if std::time::Instant::now() >= deadline => break e,
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(50)),
            }
        };
        eprintln!("decsettings: could not reuse running Settings: {error}");
        std::process::exit(1);
    };
    let listener = decebal::settings::navigation::Listener::bind(endpoint).unwrap_or_else(|e| {
        eprintln!("decsettings: navigation listener: {e}");
        std::process::exit(1);
    });
    decebal::log_info!("start: pane {:?}, rdwm config {}", pane, paths.rdwm_config.display());
    let listener = listener.for_config(&paths.rdwm_config);
    let mut app = SettingsApp::new(paths, pane.as_deref());
    app.listen_for_navigation(listener);
    if let Err(e) = run_window(app) {
        decebal::log_error!("fatal: {e}");
        eprintln!("decsettings: {e}");
        std::process::exit(1);
    }
    decebal::log_info!("window backend returned normally");
}
