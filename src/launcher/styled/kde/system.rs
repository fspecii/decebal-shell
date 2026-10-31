//! Kickoff's system actions, after plasma-workspace `applets/kicker/systementry.cpp` and `systemmodel.cpp` (commit dec158c0): ids, captions, icon names,
//! descriptions, which of them the footer shows as buttons by default (`systemFavorites` = suspend, hibernate, reboot, shutdown in plasma-desktop's
//! `applets/kickoff/main.xml`) and which sit in the Session menu. An action Plasma cannot offer is not shown (its `SystemEntry::isValid`).
//!
//! Lock, Log Out, Sleep, Hibernate, Restart and Shut Down run through Decebal's own session commands (`crate::session`, the ones `decsession` runs).
//! Save Session and Switch User need a session manager and a display manager Decebal does not have, so, as in Plasma on a system without them, they
//! are not offered. Restart, Shut Down and Log Out ask once more before they act (Plasma opens its leave screen for them).

use crate::session::{self, Choice};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sys {
    Lock,
    Logout,
    SaveSession,
    SwitchUser,
    Suspend,
    Hibernate,
    Reboot,
    Shutdown,
}

#[cfg(test)]
impl Sys {
    /// `SystemEntry::id` and `description`: the source's data, cross-checked by the tests (the footer shows no tooltips).
    pub fn id(self) -> &'static str {
        match self {
            Sys::Lock => "lock-screen",
            Sys::Logout => "logout",
            Sys::SaveSession => "save-session",
            Sys::SwitchUser => "switch-user",
            Sys::Suspend => "suspend",
            Sys::Hibernate => "hibernate",
            Sys::Reboot => "reboot",
            Sys::Shutdown => "shutdown",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Sys::Lock => "Lock screen",
            Sys::Logout => "End session",
            Sys::SaveSession => "Save Session",
            Sys::SwitchUser => "Start a parallel session as a different user",
            Sys::Suspend => "Suspend to RAM",
            Sys::Hibernate => "Suspend to disk",
            Sys::Reboot => "Restart computer",
            Sys::Shutdown => "Turn off computer",
        }
    }
}

/// SystemModel::populate order.
pub const ALL: [Sys; 8] = [Sys::Lock, Sys::Logout, Sys::SaveSession, Sys::SwitchUser, Sys::Suspend, Sys::Hibernate, Sys::Reboot, Sys::Shutdown];
/// The footer's buttons (main.xml `systemFavorites`).
pub const BUTTONS: [Sys; 4] = [Sys::Suspend, Sys::Hibernate, Sys::Reboot, Sys::Shutdown];

impl Sys {
    pub fn label(self) -> &'static str {
        match self {
            Sys::Lock => "Lock",
            Sys::Logout => "Log Out",
            Sys::SaveSession => "Save Session",
            Sys::SwitchUser => "Switch User",
            Sys::Suspend => "Sleep",
            Sys::Hibernate => "Hibernate",
            Sys::Reboot => "Restart",
            Sys::Shutdown => "Shut Down",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Sys::Lock => "system-lock-screen",
            Sys::Logout => "system-log-out",
            Sys::SaveSession => "system-save-session",
            Sys::SwitchUser => "system-switch-user",
            Sys::Suspend => "system-suspend",
            Sys::Hibernate => "system-suspend-hibernate",
            Sys::Reboot => "system-reboot",
            Sys::Shutdown => "system-shutdown",
        }
    }

    /// `requestLogout` / `requestReboot` / `requestShutdown` open Plasma's leave screen; here they take a second press.
    pub fn confirms(self) -> bool {
        matches!(self, Sys::Logout | Sys::Reboot | Sys::Shutdown)
    }

    fn choice(self) -> Option<Choice> {
        Some(match self {
            Sys::Lock => Choice::Lock,
            Sys::Logout => Choice::Logout,
            Sys::Suspend => Choice::Suspend,
            Sys::Hibernate => Choice::Hibernate,
            Sys::Reboot => Choice::Restart,
            Sys::Shutdown => Choice::Shutdown,
            Sys::SaveSession | Sys::SwitchUser => return None,
        })
    }
}

/// `SystemEntry::isValid`, probed once when the launcher is built (`/sys/power/state` offers `disk` for Hibernate).
pub fn valid(sys: Sys, hibernate_possible: bool) -> bool {
    match sys {
        Sys::SaveSession | Sys::SwitchUser => false,
        Sys::Hibernate => hibernate_possible,
        _ => true,
    }
}

pub fn probe_hibernate() -> bool {
    !cfg!(test) && session::hibernate_available()
}

#[cfg(test)]
thread_local! {
    /// What the unit tests "ran": nothing ever reaches systemctl or loginctl from a test.
    pub static RAN: std::cell::RefCell<Vec<Sys>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Runs the action (detached). True when a command was started.
pub fn run(sys: Sys) -> bool {
    #[cfg(test)]
    {
        RAN.with(|r| r.borrow_mut().push(sys));
        sys.choice().is_some()
    }
    #[cfg(not(test))]
    {
        match sys.choice() {
            Some(c) => {
                let argv = session::command(c, &session::Env::detect());
                session::spawn_detached(&argv)
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_captions_icons_and_descriptions_are_the_sources() {
        let rows: Vec<_> = ALL.iter().map(|s| (s.id(), s.label(), s.icon(), s.description())).collect();
        assert_eq!(rows[0], ("lock-screen", "Lock", "system-lock-screen", "Lock screen"));
        assert_eq!(rows[4], ("suspend", "Sleep", "system-suspend", "Suspend to RAM"));
        assert_eq!(rows[5], ("hibernate", "Hibernate", "system-suspend-hibernate", "Suspend to disk"));
        assert_eq!(rows[6], ("reboot", "Restart", "system-reboot", "Restart computer"));
        assert_eq!(rows[7], ("shutdown", "Shut Down", "system-shutdown", "Turn off computer"));
        assert_eq!(BUTTONS.map(Sys::id), ["suspend", "hibernate", "reboot", "shutdown"]);
    }

    #[test]
    fn actions_plasma_would_hide_are_not_offered_and_only_leaving_ones_confirm() {
        assert!(!valid(Sys::SaveSession, true) && !valid(Sys::SwitchUser, true));
        assert!(valid(Sys::Hibernate, true) && !valid(Sys::Hibernate, false));
        assert!(valid(Sys::Suspend, false) && valid(Sys::Lock, false));
        let confirming: Vec<_> = ALL.iter().filter(|s| s.confirms()).map(|s| s.id()).collect();
        assert_eq!(confirming, ["logout", "reboot", "shutdown"]);
        assert!(!run(Sys::SaveSession), "nothing implements Save Session here");
    }
}
