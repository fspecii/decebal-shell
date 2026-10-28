//! Test helper: starts a desktop entry through decebal's own launch code, then waits for it to be
//! reaped (so early-exit reporting runs). Usage: `launchapp DESKTOP_ID [SECONDS_TO_WAIT]`.
fn main() {
    let id = std::env::args().nth(1).expect("desktop id");
    let secs: u64 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(1);
    let ok = decebal::launch::launch(&id);
    println!("launch({id}) = {ok}");
    std::thread::sleep(std::time::Duration::from_secs(secs));
    std::process::exit(if ok { 0 } else { 1 });
}
