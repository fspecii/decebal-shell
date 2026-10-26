//! X11 backend placeholder; implemented next.
use super::SurfaceSpec;
use crate::app::App;
pub fn run<A: App>(_app: A, _spec: SurfaceSpec) -> Result<(), String> { Err("x11 backend not implemented yet".into()) }
