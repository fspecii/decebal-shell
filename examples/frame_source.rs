//! Native QA helper: a custom Source must never publish to an unrelated desktop.
use decebal::theme::{frame, live::Source};
fn main() {
    let mut args = std::env::args_os().skip(1);
    let first = args.next().expect("custom shell JSON or --runtime");
    if first == "--runtime" {
        let path = args.next().expect("runtime socket");
        let before = decebal::settings::io::stamp(std::path::Path::new(&path));
        let raw = decebal::settings::io::ipc_call(std::path::Path::new(&path), &serde_json::json!({"cmd":"get_state"}));
        let after = decebal::settings::io::stamp(std::path::Path::new(&path));
        let rt = decebal::settings::io::poll_runtime(std::path::Path::new(&path));
        println!(
            "{}",
            serde_json::json!({"before":format!("{before:?}"),"after":format!("{after:?}"),"raw_error":raw.err(),"running":rt.running,"effects":rt.effects.map(|e|serde_json::json!({"protocol":e.protocol,"backend":e.backend,"instance":e.instance,"ssd_shadows":e.ssd_shadows})),"decoration":rt.config["decoration"]})
        );
        return;
    }
    let src = Source { shell_json: first.into(), themes_dir: args.next().expect("custom themes directory").into() };
    assert!(args.next().is_none());
    let (theme, problem) = src.build(&src.selection(), true);
    assert!(problem.is_none(), "custom fixture must resolve");
    println!("{}", theme.background.hex());
    frame::publish_committed(&src, true);
}
