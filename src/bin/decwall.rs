//! Live wallpaper controller and session host.
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = if args.first().map(String::as_str) == Some("daemon") { decebal::livewall::host::run().map(|_| String::new()) } else { decebal::livewall::cli(&args) };
    match result {
        Ok(s) => {
            if !s.is_empty() {
                println!("{s}");
            }
        }
        Err(e) => {
            eprintln!("decwall: {e}");
            std::process::exit(1);
        }
    }
}
