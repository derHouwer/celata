// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! Running in the background with the icon in the system tray: what closing the window does, how Celata
//! starts at login, and when the window has to come back because the icon is gone. Celata must never run
//! on without a way back to it.

use std::time::{Duration, Instant};

use crate::presence::TrayPresence;
use crate::state::{CloseDecision, CloseDialog};

/// Longest wait for the system tray after a start at login before the window appears.
pub const TRAY_WAIT: Duration = Duration::from_secs(30);
/// The window comes back when the icon has been missing this long while the window is hidden.
pub const RESTORE_AFTER: Duration = Duration::from_secs(10);

/// What closing the window does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloseOutcome {
    /// Hide the window; Celata, the connection and the icon stay.
    Hide,
    /// End Celata at once.
    Close,
    /// Ask first, as without running in the background.
    Ask(CloseDialog),
}

/// Closing the window: with "Keep running after closing" on and the icon shown, only hide it. Otherwise the
/// usual decision, because without an icon nothing would lead back to Celata.
pub fn on_close(keep_running: bool, presence: TrayPresence, decision: CloseDecision) -> CloseOutcome {
    if keep_running && presence == TrayPresence::Shown {
        return CloseOutcome::Hide;
    }
    match decision {
        CloseDecision::Close => CloseOutcome::Close,
        CloseDecision::Ask(dialog) => CloseOutcome::Ask(dialog),
    }
}

/// How the window appears at the start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartMode {
    /// Show the window at once.
    Window,
    /// Started at login with "Keep running after closing": no window as long as the icon appears within `wait`.
    TrayOnly { wait: Duration },
}

/// Only a start at login (`--autostart`) with "Keep running after closing" starts without the window.
pub fn start_mode(autostart: bool, keep_running: bool) -> StartMode {
    if autostart && keep_running {
        StartMode::TrayOnly { wait: TRAY_WAIT }
    } else {
        StartMode::Window
    }
}

/// The window was hidden by Celata and the icon has not been shown since `absent_since` for at least
/// [`RESTORE_AFTER`]: bring the window back.
pub fn should_restore(hidden: bool, absent_since: Option<Instant>, now: Instant) -> bool {
    hidden && absent_since.is_some_and(|since| now.saturating_duration_since(since) >= RESTORE_AFTER)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Key;

    fn dialog() -> CloseDialog {
        CloseDialog {
            text: Key::CloseConnectedText,
            offers_disconnect: true,
            keep_label: Key::CloseKeepConnected,
            command: None,
        }
    }

    #[test]
    fn schliessen_verbirgt_nur_mit_schalter_und_sichtbarem_symbol() {
        assert_eq!(
            on_close(true, TrayPresence::Shown, CloseDecision::Close),
            CloseOutcome::Hide
        );
        assert_eq!(
            on_close(true, TrayPresence::Shown, CloseDecision::Ask(dialog())),
            CloseOutcome::Hide
        );
    }

    #[test]
    fn ohne_symbol_oder_ohne_schalter_wie_bisher() {
        for presence in [TrayPresence::Absent, TrayPresence::WatcherOnly, TrayPresence::Unknown] {
            assert_eq!(on_close(true, presence, CloseDecision::Close), CloseOutcome::Close);
            assert_eq!(
                on_close(true, presence, CloseDecision::Ask(dialog())),
                CloseOutcome::Ask(dialog())
            );
        }
        for presence in [TrayPresence::Shown, TrayPresence::Absent] {
            assert_eq!(on_close(false, presence, CloseDecision::Close), CloseOutcome::Close);
            assert_eq!(
                on_close(false, presence, CloseDecision::Ask(dialog())),
                CloseOutcome::Ask(dialog())
            );
        }
    }

    #[test]
    fn start_ohne_fenster_nur_bei_anmeldung_mit_hintergrundbetrieb() {
        assert_eq!(start_mode(false, false), StartMode::Window);
        assert_eq!(start_mode(false, true), StartMode::Window);
        assert_eq!(start_mode(true, false), StartMode::Window);
        assert_eq!(
            start_mode(true, true),
            StartMode::TrayOnly {
                wait: Duration::from_secs(30)
            }
        );
    }

    #[test]
    fn fenster_kommt_zurueck_wenn_das_symbol_fehlt() {
        let start = Instant::now();
        let later = |seconds| start + Duration::from_secs(seconds);
        assert!(!should_restore(true, None, later(60)));
        assert!(!should_restore(true, Some(start), later(9)));
        assert!(should_restore(true, Some(start), later(10)));
        assert!(should_restore(true, Some(start), later(11)));
        assert!(!should_restore(false, Some(start), later(60)));
        // A time before the start of the absence never counts.
        assert!(!should_restore(true, Some(later(5)), start));
    }
}
