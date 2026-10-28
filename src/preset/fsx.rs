//! Saving a file safely: atomic replace plus a timestamped backup next to it (`name.bak.YYYYmmdd-HHMMSS`), and a
//! one-step revert that is itself revertible (the file being replaced becomes the newest backup).

use crate::settings::io::write_atomic;
use std::path::{Path, PathBuf};

pub const KEEP_BACKUPS: usize = 20;

/// Local time as `YYYYmmdd-HHMMSS`.
pub fn timestamp() -> String {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    stamp_of(t)
}

pub fn stamp_of(unix: u64) -> String {
    // SAFETY: localtime_r fills the struct we pass; it keeps no pointer to it.
    let tm = unsafe {
        let t = unix as libc::time_t;
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        tm
    };
    format!("{:04}{:02}{:02}-{:02}{:02}{:02}", tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday, tm.tm_hour, tm.tm_min, tm.tm_sec)
}

/// UTC `YYYY-mm-ddTHH:MM:SSZ` for manifests.
pub fn iso_utc(unix: u64) -> String {
    // SAFETY: gmtime_r fills the struct we pass; it keeps no pointer to it.
    let tm = unsafe {
        let t = unix as libc::time_t;
        let mut tm: libc::tm = std::mem::zeroed();
        libc::gmtime_r(&t, &mut tm);
        tm
    };
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday, tm.tm_hour, tm.tm_min, tm.tm_sec)
}

pub fn now_iso() -> String {
    iso_utc(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0))
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// The directory a (possibly symlinked) config really lives in, and its real file name.
fn real(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Existing timestamped backups of `path`, newest first.
pub fn backups(path: &Path) -> Vec<PathBuf> {
    let target = real(path);
    let Some(dir) = target.parent() else { return Vec::new() };
    let prefix = format!("{}.bak.", file_name(&target));
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir).map(|rd| rd.flatten().filter(|e| e.file_name().to_string_lossy().starts_with(&prefix)).map(|e| e.path()).collect()).unwrap_or_default();
    // names sort chronologically (fixed-width stamp, then the collision counter)
    v.sort();
    v.reverse();
    v
}

fn backup_path(target: &Path) -> PathBuf {
    let dir = target.parent().unwrap_or(Path::new("."));
    let base = format!("{}.bak.{}", file_name(target), timestamp());
    let mut p = dir.join(&base);
    let mut n = 1;
    while p.exists() {
        n += 1;
        p = dir.join(format!("{base}-{n:03}"));
    }
    p
}

fn prune(path: &Path, keep: usize) {
    for old in backups(path).into_iter().skip(keep) {
        let _ = std::fs::remove_file(old);
    }
}

/// Writes `text` over `path` atomically. When the file exists and differs, its old content is first copied to a
/// timestamped backup, whose path is returned. Nothing is written if the content is unchanged.
pub fn save_with_backup(path: &Path, text: &str) -> Result<Option<PathBuf>, String> {
    let target = real(path);
    let old = std::fs::read(&target).ok();
    if old.as_deref() == Some(text.as_bytes()) {
        return Ok(None);
    }
    let mut made = None;
    if let Some(old) = old {
        let bak = backup_path(&target);
        std::fs::write(&bak, old).map_err(|e| format!("backup {}: {e}", bak.display()))?;
        if let Ok(m) = std::fs::metadata(&target) {
            let _ = std::fs::set_permissions(&bak, m.permissions());
        }
        made = Some(bak);
    }
    write_atomic(path, text)?;
    prune(path, KEEP_BACKUPS);
    Ok(made)
}

/// Restores the newest backup. The content being replaced is saved as a new backup first, so a second revert goes back.
pub fn revert_latest(path: &Path) -> Result<PathBuf, String> {
    let newest = backups(path).into_iter().next().ok_or("there is no earlier version of this file")?;
    let text = std::fs::read_to_string(&newest).map_err(|e| format!("{}: {e}", newest.display()))?;
    // the swap makes `newest` older than the backup of the current content
    let cur = std::fs::read_to_string(real(path)).ok();
    match cur {
        Some(c) if c != text => {
            save_with_backup(path, &text)?;
            let _ = std::fs::remove_file(&newest);
            // the new newest backup is the replaced content; keep `newest` content discoverable too: it is now the live file
        }
        Some(_) => {
            let _ = std::fs::remove_file(&newest);
            return Err("the file already equals its previous version".into());
        }
        None => write_atomic(path, &text)?,
    }
    Ok(newest)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("decebal-fsx-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn stamps_have_a_fixed_width() {
        let s = stamp_of(1_800_000_000);
        assert_eq!(s.len(), 15);
        assert_eq!(s.as_bytes()[8], b'-');
        assert_eq!(iso_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_utc(1_800_000_000), "2027-01-15T08:00:00Z");
    }

    #[test]
    fn save_makes_a_timestamped_backup_and_skips_unchanged() {
        let d = tmp("save");
        let p = d.join("decebal.json");
        assert_eq!(save_with_backup(&p, "{\"a\": 1}\n").unwrap(), None, "a new file has nothing to back up");
        assert_eq!(save_with_backup(&p, "{\"a\": 1}\n").unwrap(), None);
        let b1 = save_with_backup(&p, "{\"a\": 2}\n").unwrap().expect("backup");
        assert!(b1.file_name().unwrap().to_string_lossy().starts_with("decebal.json.bak."), "{b1:?}");
        assert_eq!(std::fs::read_to_string(&b1).unwrap(), "{\"a\": 1}\n");
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "{\"a\": 2}\n");
        // two saves within the same second never overwrite each other's backup
        let b2 = save_with_backup(&p, "{\"a\": 3}\n").unwrap().expect("backup");
        assert_ne!(b1, b2);
        assert_eq!(std::fs::read_to_string(&b2).unwrap(), "{\"a\": 2}\n");
        assert_eq!(backups(&p).len(), 2);
        assert_eq!(backups(&p)[0], b2, "newest first");
        // no temp files are left behind
        let left: Vec<_> = std::fs::read_dir(&d).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert!(!left.iter().any(|n| n.ends_with(".tmp")), "{left:?}");
    }

    #[test]
    fn revert_swaps_with_the_newest_backup() {
        let d = tmp("revert");
        let p = d.join("dock-settings.json");
        assert!(revert_latest(&p).is_err());
        std::fs::write(&p, "one\n").unwrap();
        save_with_backup(&p, "two\n").unwrap();
        save_with_backup(&p, "three\n").unwrap();
        revert_latest(&p).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "two\n");
        // reverting again goes back to "three": the revert itself was backed up
        revert_latest(&p).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "three\n");
    }

    #[test]
    fn backups_are_pruned() {
        let d = tmp("prune");
        let p = d.join("f.json");
        std::fs::write(&p, "0").unwrap();
        for i in 1..=KEEP_BACKUPS + 5 {
            save_with_backup(&p, &i.to_string()).unwrap();
        }
        assert_eq!(backups(&p).len(), KEEP_BACKUPS);
        assert_eq!(std::fs::read_to_string(&p).unwrap(), (KEEP_BACKUPS + 5).to_string());
    }

    #[test]
    fn symlinked_files_are_written_through() {
        let d = tmp("link");
        std::fs::create_dir_all(d.join("dots")).unwrap();
        std::fs::write(d.join("dots/real.json"), "a").unwrap();
        std::os::unix::fs::symlink(d.join("dots/real.json"), d.join("link.json")).unwrap();
        let b = save_with_backup(&d.join("link.json"), "b").unwrap().unwrap();
        assert!(b.starts_with(d.join("dots").canonicalize().unwrap()), "backup beside the real file: {b:?}");
        assert!(std::fs::symlink_metadata(d.join("link.json")).unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read_to_string(d.join("dots/real.json")).unwrap(), "b");
    }
}
