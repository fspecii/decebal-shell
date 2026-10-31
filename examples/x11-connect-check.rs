//! Minimal transport/authentication probe; never reads selections or sends input.
use x11rb::connection::Connection;
fn main() {
    let direct = std::env::args().any(|arg| arg == "--upstream");
    let result = if direct { x11rb::connect(None) } else { decebal::x11_connection::connect(None) };
    match result {
        Ok((conn, screen)) => println!("connected screen={screen} roots={}", conn.setup().roots.len()),
        Err(error) => {
            eprintln!("connection failed: {error}");
            std::process::exit(1);
        }
    }
}
