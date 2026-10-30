use decebal::timer::{Store, TimerApp};
fn main() {
    let result = (|| -> Result<(), String> {
        let mut args = std::env::args_os().skip(1);
        match args.next() {
            Some(a) if a == "--timer-worker" => {
                let dir = args.next().ok_or("Missing worker state directory")?;
                if args.next().is_some() {
                    return Err("Unexpected worker argument".into());
                }
                return Store::in_dir(dir).worker();
            }
            Some(a) if a == "--help" || a == "-h" => {
                println!("dectimer — native countdown timer\nUsage: dectimer [--help | --version]\nF1/F2/F3 presets; enter seconds or MM:SS. Enter/Space start/pause/resume; R reset; Esc close.\nCountdown includes suspend and time closed; notifications need notify-send.");
                return Ok(());
            }
            Some(a) if a == "--version" || a == "-V" => {
                println!("dectimer {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            Some(a) => return Err(format!("Unknown argument: {}", a.to_string_lossy())),
            None => {}
        }
        decebal::compat::init();
        let store = Store::discover()?;
        if matches!(store.snapshot()?.status, decebal::timer::Status::Running | decebal::timer::Status::Complete) {
            store.spawn_worker()?;
        }
        decebal::window_app::run_window(TimerApp::new(store))
    })();
    if let Err(e) = result {
        eprintln!("dectimer: {e}");
        std::process::exit(1)
    }
}
