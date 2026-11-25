// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! Donation hint: link to the Ko-fi page, opening it in the browser, texts.
//!
//! Celata never connects to Ko-fi itself. Only on a click it hands the fixed address to `xdg-open`,
//! which starts the default browser of the system. `tests/spendenhinweis.rs` checks that only this
//! file knows the link and that its program part contains no network access and no file access.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::i18n::Key;
use crate::state::DisplayState;

/// The Ko-fi page of the maintainer. The only place in the program; never read it from files or the
/// environment.
pub const KOFI_URL: &str = "https://ko-fi.com/derhauer";

// The texts are keys `Donation…` in `src/i18n` (both languages); the link is only here and never in
// `src/i18n` or `ui/translations/` (tests/spendenhinweis.rs).

/// Program that opens the default browser.
pub const OPENER: &str = "xdg-open";

const PREFIX: &str = "https://ko-fi.com/";
const MAX_NAME: usize = 50;
/// Names that look like a forgotten placeholder.
const PLACEHOLDERS: [&str; 5] = ["name", "dein-name", "placeholder", "platzhalter", "example"];

/// Why opening the browser did not work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenError {
    /// The program could not be started (for example because it is missing).
    Start,
    /// The program ended with this exit code other than 0.
    Exit(i32),
    /// The program was terminated by a signal.
    Signal,
}

/// Is `url` a specific Ko-fi page? Requires `https://ko-fi.com/` followed by a name of letters,
/// digits or `_` (1 to 50 characters), with no further characters and no placeholder. An own,
/// deliberately narrow rule, not the one of Ko-fi.
pub fn is_valid_kofi_url(url: &str) -> bool {
    let Some(name) = url.strip_prefix(PREFIX) else {
        return false;
    };
    (1..=MAX_NAME).contains(&name.len())
        && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        && !PLACEHOLDERS
            .iter()
            .any(|placeholder| name.eq_ignore_ascii_case(placeholder))
}

/// Starts `program` with `url` as its only argument, without a shell, and waits for it to end. Input
/// and output go nowhere. A long wait is not an error; therefore call it only in a background
/// thread.
pub fn open_with(program: &str, url: &str) -> Result<(), OpenError> {
    let status = Command::new(program)
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|_| OpenError::Start)?;
    match status.code() {
        Some(0) => Ok(()),
        Some(code) => Err(OpenError::Exit(code)),
        None => Err(OpenError::Signal),
    }
}

/// Opens `url` in the default browser through [`OPENER`].
pub fn open(url: &str) -> Result<(), OpenError> {
    open_with(OPENER, url)
}

// ---------------------------------------------------------------------------------------------------
// Feedback
// ---------------------------------------------------------------------------------------------------

/// Info message if the internet is blocked at the time of the click. The browser opens anyway.
pub const BLOCKED_NOTICE: Key = Key::DonationBlockedNotice;
/// Info message if the browser could not be opened; Celata also shows the link for copying.
pub const FAILED_NOTICE: Key = Key::DonationOpenFailed;

/// Clicks within this period after the last accepted click do not count.
pub const DEBOUNCE: Duration = Duration::from_secs(2);

/// Protection against double clicks, shared by both donation buttons.
#[derive(Debug, Default)]
pub struct Debounce {
    /// Time of the last accepted click, `None` = none yet.
    last: Option<Instant>,
}

impl Debounce {
    /// Accepts the click at time `now` if there was none yet or the last accepted one is at least
    /// [`DEBOUNCE`] ago. A rejected click does not move the period.
    pub fn allow(&mut self, now: Instant) -> bool {
        let free = self
            .last
            .is_none_or(|last| now.saturating_duration_since(last) >= DEBOUNCE);
        if free {
            self.last = Some(now);
        }
        free
    }
}

/// Message for the click: only with blocked internet a notice why the page does not load.
pub fn click_notice(blocked: bool) -> Option<Key> {
    blocked.then_some(BLOCKED_NOTICE)
}

/// Is the internet blocked according to the state shown? Short, intended blocks during a switch or a
/// reconnect do not count. Without a catch-all arm, so that a new state shows up when compiling.
pub fn is_blocked(display: Option<&DisplayState>) -> bool {
    let Some(display) = display else {
        return false;
    };
    match display {
        DisplayState::Blocked | DisplayState::ConnectingBlocked | DisplayState::ServiceUnreachableBlocked => true,
        DisplayState::ServiceStarting { blocked } => *blocked,
        DisplayState::NotConnected
        | DisplayState::Connecting
        | DisplayState::Connected { .. }
        | DisplayState::Disconnecting
        | DisplayState::Switching { .. }
        | DisplayState::Reapplying { .. }
        | DisplayState::ServiceUnreachable
        | DisplayState::NoPermission => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn hinterlegter_link_ist_gueltig() {
        assert!(is_valid_kofi_url(KOFI_URL));
    }

    #[test]
    fn pruefung_lehnt_falsche_adressen_ab() {
        let too_long = format!("https://ko-fi.com/{}", "a".repeat(51));
        let wrong = [
            "https://ko-fi.com/",
            "https://ko-fi.com",
            "http://ko-fi.com/derhauer",
            "https://ko-fi.com.evil.example/derhauer",
            "https://evil.example/ko-fi.com/derhauer",
            "https://ko-fi.com/derhauer/",
            "https://ko-fi.com/derhauer?ref=celata",
            "https://ko-fi.com/derhauer#x",
            "https://ko-fi.com/der hauer",
            "https://ko-fi.com/der-hauer",
            too_long.as_str(),
        ];
        for url in wrong {
            assert!(!is_valid_kofi_url(url), "{url} darf nicht gültig sein");
        }
        let fifty = format!("https://ko-fi.com/{}", "a".repeat(50));
        assert!(is_valid_kofi_url(&fifty), "50 Zeichen sind erlaubt");
    }

    #[test]
    fn pruefung_lehnt_platzhalter_ab() {
        for name in ["name", "dein-name", "placeholder", "platzhalter", "example"] {
            for written in [name.to_owned(), name.to_uppercase()] {
                let url = format!("https://ko-fi.com/{written}");
                assert!(!is_valid_kofi_url(&url), "{url} ist ein Platzhalter");
            }
        }
        assert!(!is_valid_kofi_url("https://ko-fi.com/Example"));
    }

    #[test]
    fn erfolgreiches_oeffnen() {
        assert_eq!(open_with("/usr/bin/true", KOFI_URL), Ok(()));
    }

    #[test]
    fn rueckgabewert_ungleich_null_ist_ein_fehler() {
        assert_eq!(open_with("/usr/bin/false", KOFI_URL), Err(OpenError::Exit(1)));
    }

    #[test]
    fn fehlendes_programm_ist_ein_fehler() {
        assert_eq!(open_with("/nicht/vorhanden/xdg-open", KOFI_URL), Err(OpenError::Start));
    }

    // File access is only here in the test module; `tests/spendenhinweis.rs` checks the part before it.
    #[test]
    fn link_geht_unveraendert_als_einziges_argument_weiter() {
        let dir = std::env::temp_dir().join(format!("celata-spende-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let script = dir.join("oeffner");
        fs::write(&script, "#!/bin/sh\nprintf '%s\\n' \"$#\" \"$@\" > \"$0.args\"\n").unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
        // If another test is just starting a program, its child process keeps the script that was just written
        // open for a moment; the start then fails with "text file busy". Therefore retry briefly.
        let mut result = open_with(script.to_str().unwrap(), KOFI_URL);
        for _ in 0..100 {
            if result != Err(OpenError::Start) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
            result = open_with(script.to_str().unwrap(), KOFI_URL);
        }
        assert_eq!(result, Ok(()));
        let args = fs::read_to_string(dir.join("oeffner.args")).unwrap();
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(args, format!("1\n{KOFI_URL}\n"));
    }

    #[test]
    fn erster_klick_wird_angenommen() {
        let mut debounce = Debounce::default();
        assert!(debounce.allow(Instant::now()));
    }

    #[test]
    fn zweiter_klick_innerhalb_von_zwei_sekunden_zaehlt_nicht() {
        let start = Instant::now();
        let mut debounce = Debounce::default();
        assert!(debounce.allow(start));
        assert!(!debounce.allow(start + Duration::from_secs(1)));
        assert!(debounce.allow(start + Duration::from_secs(2)));
        assert!(!debounce.allow(start + Duration::from_millis(3_999)));
        assert!(debounce.allow(start + Duration::from_secs(4)));
    }

    #[test]
    fn abgelehnter_klick_verschiebt_die_frist_nicht() {
        let start = Instant::now();
        let mut debounce = Debounce::default();
        assert!(debounce.allow(start));
        assert!(!debounce.allow(start + Duration::from_millis(1_500)));
        assert!(debounce.allow(start + Duration::from_millis(2_000)));
    }

    #[test]
    fn meldung_nur_bei_sperre() {
        assert_eq!(click_notice(true), Some(BLOCKED_NOTICE));
        assert_eq!(click_notice(false), None);
    }

    #[test]
    fn gesperrt_sind_genau_drei_zustaende() {
        let blocked = [
            DisplayState::Blocked,
            DisplayState::ConnectingBlocked,
            DisplayState::ServiceUnreachableBlocked,
        ];
        for state in &blocked {
            assert!(is_blocked(Some(state)), "{state:?} gilt als gesperrt");
        }
        let free = [
            DisplayState::NotConnected,
            DisplayState::Connecting,
            DisplayState::Connected { server: None },
            DisplayState::Disconnecting,
            DisplayState::Switching {
                target: "x".into(),
                back: false,
            },
            DisplayState::Reapplying { back: false },
            DisplayState::ServiceUnreachable,
            DisplayState::NoPermission,
        ];
        for state in &free {
            assert!(!is_blocked(Some(state)), "{state:?} gilt nicht als gesperrt");
        }
        assert!(!is_blocked(None));
        assert_eq!(blocked.len() + free.len(), 11, "jeder Zustand kommt vor");
    }

    #[test]
    fn texte_der_meldungen() {
        use crate::i18n::{Language, text};
        assert_eq!(
            text(Language::De, BLOCKED_NOTICE),
            "Das Internet ist gesperrt. Die Seite lädt, sobald das VPN verbunden oder die Sperre aufgehoben ist."
        );
        assert_eq!(
            text(Language::De, FAILED_NOTICE),
            "Der Browser ließ sich nicht öffnen. Du kannst den Link kopieren und selbst im Browser öffnen."
        );
        assert!(text(Language::En, BLOCKED_NOTICE).starts_with("The internet is blocked."));
    }

    #[test]
    fn texte_der_knoepfe() {
        use crate::i18n::{Language, text};
        assert_eq!(text(Language::De, Key::DonationButton), "Spenden über Ko-fi");
        assert_eq!(
            text(Language::De, Key::DonationButtonLabel),
            "Spenden über Ko-fi, öffnet den Browser"
        );
        assert_eq!(text(Language::En, Key::DonationButton), "Donate via Ko-fi");
    }
}
