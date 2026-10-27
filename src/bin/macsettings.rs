//! macsettings: System Settings for rdwm and the macshell panels. Works on Wayland and X11.

use macshell::settings::app::SettingsApp;
use macshell::settings::io::Paths;
use macshell::window_app::run_window;

const HELP: &str = "macsettings [OPTIONS] [PANE]

Settings for the rdwm window manager and the macshell dock and menu bar. Edits ~/.config/rdwm/config.toml
in place (comments and unknown keys are kept), applies changes live, and follows edits made by hand.

PANE is one of: general appearance desktop wallpaper tiling windows rules keyboard autostart displays

Options:
  --config PATH   rdwm config file to edit (default: $MACSETTINGS_CONFIG, $XDG_CONFIG_HOME/rdwm/config.toml)
  -h, --help      show this help
  -V, --version   show the version

Keys: Tab/Shift+Tab move, arrows adjust, Space/Enter activate, Ctrl+F search, Esc or Ctrl+W close.
Environment: MACSETTINGS_SCALE (UI scale), MACSETTINGS_CONFIG, MACSHELL_CONFIG (folder with dock-settings.json),
RDWMCTL (path of rdwmctl), RDWM_SOCK.";

fn main() {
    macshell::mem::limit_arenas();
    let mut pane: Option<String> = None;
    let mut paths = Paths::discover();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-h" | "--help" => {
                println!("{HELP}");
                return;
            }
            "-V" | "--version" => {
                println!("macsettings {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "--config" => match args.next() {
                Some(p) => paths.rdwm_config = p.into(),
                None => {
                    eprintln!("macsettings: --config needs a path");
                    std::process::exit(2);
                }
            },
            s if s.starts_with('-') => {
                eprintln!("macsettings: unknown option {s}\n\n{HELP}");
                std::process::exit(2);
            }
            _ => pane = Some(a),
        }
    }
    // Launching it again toggles the running window closed, like the other macshell windows.
    let Some(_guard) = macshell::single::acquire("macsettings") else {
        return;
    };
    if let Err(e) = run_window(SettingsApp::new(paths, pane.as_deref())) {
        eprintln!("macsettings: {e}");
        std::process::exit(1);
    }
}
