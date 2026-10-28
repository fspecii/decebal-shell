//! decsettings: System Settings for rdwm and the decebal panels. Works on Wayland and X11.

use decebal::settings::app::SettingsApp;
use decebal::settings::io::Paths;
use decebal::window_app::run_window;

const HELP: &str = "decsettings [OPTIONS] [PANE]

Settings for the rdwm window manager and the decebal dock and menu bar. Edits ~/.config/rdwm/config.toml
in place (comments and unknown keys are kept), applies changes live, and follows edits made by hand.

PANE is one of: general appearance tiling windows rules keyboard autostart displays config (and the system panes: sound network
bluetooth notifications inputdev power defaults datetime users privacy storage accessibility). `appearance` is the home of how the
desktop looks (tabs: Theme, Customise, Wallpaper, All apps). These older names open their tab: theme, icons, wallpaper, desktop;
also appearance/fonts, appearance/window, appearance/dock, appearance/colours, appearance/apps. config = Config & Presets (edit the
files as text, save/apply/export/import presets, see docs/presets.md).

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
                Some(p) => paths.rdwm_config = p.into(),
                None => {
                    eprintln!("decsettings: --config needs a path");
                    std::process::exit(2);
                }
            },
            s if s.starts_with('-') => {
                eprintln!("decsettings: unknown option {s}\n\n{HELP}");
                std::process::exit(2);
            }
            _ => pane = Some(a),
        }
    }
    // Launching it again toggles the running window closed, like the other decebal windows.
    let Some(_guard) = decebal::single::acquire("decsettings") else {
        return;
    };
    if let Err(e) = run_window(SettingsApp::new(paths, pane.as_deref())) {
        eprintln!("decsettings: {e}");
        std::process::exit(1);
    }
}
