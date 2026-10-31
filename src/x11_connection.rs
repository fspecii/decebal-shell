//! Shared X11 connector, including Linux abstract Unix sockets.
#[cfg(target_os = "linux")]
use x11rb::reexports::x11rb_protocol::{parse_display, xauth};
use x11rb::{
    errors::ConnectError,
    rust_connection::{DefaultStream, RustConnection},
};

#[cfg(target_os = "linux")]
fn local_abstract_path(display: &parse_display::ParsedDisplay) -> Option<String> {
    // Follow the upstream transport routing; explicit remote/TCP displays must
    // never accidentally connect to a local server with the same number.
    if display.host.starts_with('/') {
        return None;
    }
    match (display.protocol.as_deref(), display.host.as_str()) {
        (None, "" | "unix") | (Some("unix"), _) => Some(format!("/tmp/.X11-unix/X{}", display.display)),
        _ => None,
    }
}

/// Connect using the same DISPLAY, screen and Xauthority rules as x11rb.
/// Fall back only when the abstract transport cannot be established. Once a
/// server accepts the transport, its authentication/setup result is final.
pub fn connect(display: Option<&str>) -> Result<(RustConnection<DefaultStream>, usize), ConnectError> {
    // Capture DISPLAY once so the abstract attempt and fallback cannot differ.
    let captured = match display {
        Some(value) => value.to_owned(),
        None => std::env::var("DISPLAY").map_err(|error| match error {
            std::env::VarError::NotPresent => x11rb::errors::DisplayParsingError::DisplayNotSet,
            std::env::VarError::NotUnicode(_) => x11rb::errors::DisplayParsingError::NotUnicode,
        })?,
    };
    #[cfg(target_os = "linux")]
    {
        use std::os::{
            linux::net::SocketAddrExt,
            unix::net::{SocketAddr, UnixStream},
        };
        let parsed = parse_display::parse_display(Some(&captured))?;
        if let Some(path) = local_abstract_path(&parsed) {
            if let Ok(stream) = SocketAddr::from_abstract_name(path.as_bytes()).and_then(|addr| UnixStream::connect_addr(&addr)) {
                let (stream, (family, address)) = DefaultStream::from_unix_stream(stream)?;
                let (name, data) = xauth::get_auth(family, &address, parsed.display).unwrap_or(None).unwrap_or_default();
                let screen = usize::from(parsed.screen);
                let connection = RustConnection::connect_to_stream_with_auth_info(stream, screen, name, data)?;
                return Ok((connection, screen));
            }
        }
    }
    x11rb::connect(Some(&captured))
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    #[test]
    fn local_transport_and_screen() {
        for input in [":31.2", "unix/:31.2", "unix/remote:31.2"] {
            let parsed = parse_display::parse_display(Some(input)).unwrap();
            assert_eq!(local_abstract_path(&parsed).as_deref(), Some("/tmp/.X11-unix/X31"));
            assert_eq!(parsed.screen, 2);
        }
    }
    #[test]
    fn remote_and_explicit_protocols_stay_remote() {
        for input in ["remote:31", "localhost:31", "tcp/localhost:31", "tcp/:31", "inet/remote:31", "[::1]:31"] {
            let parsed = parse_display::parse_display(Some(input)).unwrap();
            assert_eq!(local_abstract_path(&parsed), None, "{input}");
        }
    }
    #[test]
    fn explicit_socket_paths_are_not_standard_local_displays() {
        let parsed = parse_display::ParsedDisplay { host: "/private/server.sock".into(), protocol: Some("unix".into()), display: 0, screen: 3 };
        assert_eq!(local_abstract_path(&parsed), None);
    }
}
