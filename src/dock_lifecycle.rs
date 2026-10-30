//! Run the dock only while the active theme/settings require it.
use std::path::Path;

pub fn reconcile(shell_json: &Path) -> Result<(), String> {
    // Offline theme previews and temporary config transactions must not affect the session.
    if cfg!(test) || shell_json != crate::config::config_dir().join("decebal.json") || std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
        return Ok(());
    }
    reconcile_locked(|| crate::single::hold("decdock-lifecycle"), std::time::Duration::from_secs(2), reconcile_session)
}

// Read the desired state only after acquiring the lock: a waiting apply must use
// the newest saved theme, rather than replay an older request.
fn reconcile_locked<G>(mut acquire: impl FnMut() -> Option<G>, timeout: std::time::Duration, apply: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
    let start = std::time::Instant::now();
    let _guard = loop {
        if let Some(guard) = acquire() {
            break guard;
        }
        if start.elapsed() >= timeout {
            return Err("Could not acquire decdock-lifecycle lock; retry applying the theme to reconcile the dock".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    apply()
}

fn reconcile_session() -> Result<(), String> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    };

    #[test]
    fn contended_reconcile_reads_latest_desired_state_and_keeps_one_dock() {
        for latest in [false, true] {
            let dir = std::env::temp_dir().join(format!("decebal-dock-contention-{}-{latest}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join("desired");
            std::fs::write(&path, "true").unwrap();
            let role = format!("decebal-private-dock-{}-{latest}", std::process::id());
            let held = crate::single::hold(&role).unwrap();
            let attempted = Arc::new(AtomicBool::new(false));
            let dock_count = Arc::new(Mutex::new(1));
            let worker = {
                let (role, attempted, dock_count, path) = (role.clone(), attempted.clone(), dock_count.clone(), path.clone());
                std::thread::spawn(move || {
                    reconcile_locked(
                        || {
                            attempted.store(true, Ordering::SeqCst);
                            crate::single::hold(&role)
                        },
                        std::time::Duration::from_secs(1),
                        || {
                            let wanted = std::fs::read_to_string(path).unwrap() == "true";
                            *dock_count.lock().unwrap() = usize::from(wanted);
                            Ok(())
                        },
                    )
                })
            };
            while !attempted.load(Ordering::SeqCst) {
                std::thread::yield_now();
            }
            std::fs::write(path, latest.to_string()).unwrap();
            drop(held);
            worker.join().unwrap().unwrap();
            assert_eq!(*dock_count.lock().unwrap(), usize::from(latest));
            std::fs::remove_dir_all(dir).unwrap();
            std::fs::remove_file(crate::single::runtime_file(&format!("{role}.lock"))).unwrap();
        }
    }

    #[test]
    fn contention_timeout_is_actionable_and_does_not_apply() {
        let error = reconcile_locked(|| None::<()>, std::time::Duration::ZERO, || panic!("must not apply without lock")).unwrap_err();
        assert!(error.contains("decdock-lifecycle") && error.contains("retry applying"));
    }
}
