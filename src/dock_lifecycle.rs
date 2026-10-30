//! Run the dock only while the active theme/settings require it.
use std::path::Path;

pub fn reconcile(shell_json: &Path) -> Result<(), String> {
    // Offline theme previews and temporary config transactions must not affect the session.
    if cfg!(test) || shell_json != crate::config::config_dir().join("decebal.json") || std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
        return Ok(());
    }
    let Some(_guard) = crate::single::hold("decdock-lifecycle") else { return Ok(()) };
    let wanted = crate::menubar::gate::dock_visible_now();
    let supervisor = crate::single::runtime_file("rdwm-decebal");
    if supervisor.is_dir() {
        if wanted {
            // Older supervisors permanently suppress a service after a clean early exit.
            for suffix in ["off", "rc", "start", "next", "fails"] {
                let _ = std::fs::remove_file(supervisor.join(format!("decdock.{suffix}")));
            }
        } else {
            std::fs::write(supervisor.join("decdock.off"), b"").map_err(|e| e.to_string())?;
        }
    }
    if wanted {
        if !crate::single::signal("decdock", 0) && !crate::launch::spawn_argv(&[crate::launch::sibling_or_path("decdock")]) {
            return Err("Could not start the theme's dock".into());
        }
    } else {
        crate::single::signal("decdock", libc::SIGTERM);
    }
    Ok(())
}
