//! Ending processes safely: what may be signalled, what needs an extra confirmation, and the signal itself, delivered
//! through a pidfd after the process' start time was checked so a recycled pid is never hit.

use super::proc::{start_time, Proc};
use std::path::Path;

/// Programs that carry the desktop session: ending one usually ends everything on screen.
const SESSION: [&str; 9] = ["rdwm", "decbar", "decdock", "decnotify", "decwm", "Hyprland", "sway", "Xorg", "Xwayland"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ctx {
    pub my_uid: u32,
    /// This program's own pid.
    pub self_pid: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Guard {
    Allowed,
    /// May be signalled once the user has confirmed this warning.
    Confirm(&'static str),
    Refused(&'static str),
}

pub fn guard(p: &Proc, ctx: Ctx) -> Guard {
    if p.pid == 1 || p.pid == 0 {
        return Guard::Refused("PID 1 is the init system: stopping it would take the whole system down.");
    }
    if p.pid == ctx.self_pid {
        return Guard::Refused("That is this window.");
    }
    if p.uid != ctx.my_uid && ctx.my_uid != 0 {
        return Guard::Refused("It belongs to another user.");
    }
    if p.kthread {
        return Guard::Confirm("This is a kernel thread: signals do nothing to it, and its owner is the kernel.");
    }
    if SESSION.contains(&p.comm.as_str()) {
        return Guard::Confirm("This part of your desktop session (window manager, bar, dock or display server): ending it can close everything you have open.");
    }
    if p.comm == "systemd" || p.comm == "(sd-pam)" {
        return Guard::Confirm("This is the systemd user manager: ending it stops your whole login session.");
    }
    Guard::Allowed
}

/// What to do with several processes (an app and its children): who gets the signal and whether to ask first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub targets: Vec<(u32, u64)>,
    pub confirm: Option<&'static str>,
    /// The first reason something was left out, when anything was.
    pub refused: Option<&'static str>,
}

pub fn plan(procs: &[&Proc], ctx: Ctx) -> Plan {
    let mut plan = Plan { targets: Vec::new(), confirm: None, refused: None };
    for p in procs {
        match guard(p, ctx) {
            Guard::Allowed => plan.targets.push(p.key()),
            Guard::Confirm(m) => {
                plan.confirm.get_or_insert(m);
                plan.targets.push(p.key());
            }
            Guard::Refused(m) => {
                plan.refused.get_or_insert(m);
            }
        }
    }
    plan
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendError {
    /// Already gone.
    Gone,
    /// The pid now belongs to another process than the one that was listed.
    Reused,
    Denied(String),
}

/// Sends `sig` to the process `(pid, start)` of the `/proc` at `root`. The start time is read after the pidfd was opened,
/// so what the signal reaches is exactly the process that was checked.
pub fn send(root: &Path, key: (u32, u64), sig: i32) -> Result<(), SendError> {
    let (pid, start) = key;
    // SAFETY: pidfd_open takes a pid and flags and returns a new descriptor or -1.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid as libc::pid_t, 0) } as i32;
    if fd >= 0 {
        let r = match start_time(root, pid) {
            None => Err(SendError::Gone),
            Some(s) if s != start => Err(SendError::Reused),
            // SAFETY: `fd` is a pidfd we own; a null siginfo and 0 flags are valid.
            Some(_) => match unsafe { libc::syscall(libc::SYS_pidfd_send_signal, fd, sig, std::ptr::null::<libc::siginfo_t>(), 0) } {
                0 => Ok(()),
                _ => Err(errno_error()),
            },
        };
        // SAFETY: closing the descriptor opened above.
        unsafe { libc::close(fd) };
        return r;
    }
    if std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
        return Err(SendError::Gone);
    }
    // No pidfd support (an old kernel): check, then kill; the window for a pid reuse is microseconds.
    match start_time(root, pid) {
        None => Err(SendError::Gone),
        Some(s) if s != start => Err(SendError::Reused),
        // SAFETY: kill(2) with a pid we just verified.
        Some(_) => match unsafe { libc::kill(pid as libc::pid_t, sig) } {
            0 => Ok(()),
            _ => Err(errno_error()),
        },
    }
}

fn errno_error() -> SendError {
    let e = std::io::Error::last_os_error();
    match e.raw_os_error() {
        Some(libc::ESRCH) => SendError::Gone,
        _ => SendError::Denied(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CTX: Ctx = Ctx { my_uid: 1000, self_pid: 4000 };

    fn proc(pid: u32, comm: &str, uid: u32) -> Proc {
        Proc { pid, start: 7, uid, comm: comm.into(), name: comm.into(), ..Proc::default() }
    }

    #[test]
    fn init_self_and_other_users_are_refused() {
        assert!(matches!(guard(&proc(1, "systemd", 0), CTX), Guard::Refused(_)));
        assert!(matches!(guard(&proc(1, "systemd", 1000), CTX), Guard::Refused(_)), "pid 1 whoever owns it");
        assert!(matches!(guard(&proc(4000, "decmon", 1000), CTX), Guard::Refused(_)));
        assert!(matches!(guard(&proc(5, "nginx", 33), CTX), Guard::Refused(_)));
        assert_eq!(guard(&proc(5, "nginx", 33), Ctx { my_uid: 0, ..CTX }), Guard::Allowed, "root may signal anyone");
        assert!(matches!(guard(&proc(1, "systemd", 0), Ctx { my_uid: 0, ..CTX }), Guard::Refused(_)), "even root cannot end init here");
    }

    #[test]
    fn session_programs_and_kernel_threads_need_a_confirmation() {
        for c in ["rdwm", "decbar", "Hyprland", "decdock", "systemd"] {
            assert!(matches!(guard(&proc(50, c, 1000), CTX), Guard::Confirm(_)), "{c}");
        }
        let mut k = proc(60, "kworker/0:1", 0);
        k.kthread = true;
        assert!(matches!(guard(&k, Ctx { my_uid: 0, ..CTX }), Guard::Confirm(_)));
        assert!(matches!(guard(&k, CTX), Guard::Refused(_)), "someone else's");
        assert_eq!(guard(&proc(70, "firefox", 1000), CTX), Guard::Allowed);
    }

    #[test]
    fn a_plan_skips_the_refused_and_collects_the_confirm() {
        let (a, b, c, d) = (proc(10, "firefox", 1000), proc(11, "rdwm", 1000), proc(12, "x", 0), proc(1, "systemd", 0));
        let p = plan(&[&a, &c, &d], CTX);
        assert_eq!(p.targets, vec![(10, 7)]);
        assert!(p.refused.is_some() && p.confirm.is_none());
        let p = plan(&[&a, &b], CTX);
        assert_eq!(p.targets.len(), 2);
        assert!(p.confirm.is_some() && p.refused.is_none());
        assert!(plan(&[&c], CTX).targets.is_empty());
    }

    #[test]
    fn a_changed_start_time_is_never_signalled() {
        let root = std::env::temp_dir().join(format!("decmon-act-{}", std::process::id()));
        let me = std::process::id();
        std::fs::create_dir_all(root.join(me.to_string())).unwrap();
        // Our own pid, but the fixture says it started at 555: "the listed process was another one".
        std::fs::write(root.join(me.to_string()).join("stat"), format!("{me} (x) S 1 1 1 0 -1 0 0 0 0 0 0 0 0 0 20 0 1 0 555 0 0 0")).unwrap();
        assert_eq!(send(&root, (me, 1), libc::SIGTERM), Err(SendError::Reused), "this test would have killed itself otherwise");
        assert_eq!(send(&root, (me, 555), 0), Ok(()), "signal 0 only checks that the process exists");
        let _ = std::fs::remove_dir_all(&root);
        // A pid that does not exist.
        assert_eq!(send(Path::new("/nonexistent"), (u32::MAX - 1, 1), libc::SIGTERM), Err(SendError::Gone));
    }
}
