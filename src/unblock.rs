// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! `celata --unblock`: lifts a block of the internet without the window and without root, the same way as
//! "Unblock internet" in the window (`/destroy`). A connection without a block stays untouched; if the routing
//! table cannot be read, nothing is called at all.

use std::io;
use std::thread;
use std::time::{Duration, Instant};

use crate::daemon::{Client, Error, ServiceState};
use crate::i18n::{self, Key, Language, Phrase};
use crate::state::{self, DisplayState, Update};

/// What `--unblock` achieved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnblockOutcome {
    /// The block is gone.
    Freed,
    /// There was no block; nothing was changed.
    NotBlocked,
    /// The routing table cannot be read; nothing was called.
    Unreadable,
    /// The service is unreachable, refuses, or the block stays; with the reason.
    Failed(Phrase),
}

/// The calls `--unblock` needs from the service.
pub trait Service {
    fn state(&self) -> Result<ServiceState, Error>;
    /// Lifts the block (`/destroy`), as after the question in the window.
    fn unblock(&self) -> Result<(), Error>;
}

impl Service for Client {
    fn state(&self) -> Result<ServiceState, Error> {
        Client::state(self)
    }

    fn unblock(&self) -> Result<(), Error> {
        state::run_unblock(self)
    }
}

/// How long and how often the routes are checked after lifting the block.
#[derive(Debug, Clone, Copy)]
pub struct Wait {
    pub total: Duration,
    pub step: Duration,
}

impl Default for Wait {
    fn default() -> Self {
        Self {
            total: Duration::from_secs(3),
            step: Duration::from_millis(250),
        }
    }
}

/// Lifts the block if there is one. `read_routes` answers whether the routes block the internet.
pub fn unblock(service: &impl Service, read_routes: impl Fn() -> io::Result<bool>, wait: Wait) -> UnblockOutcome {
    let Ok(routes) = read_routes() else {
        return UnblockOutcome::Unreadable;
    };
    let state = match service.state() {
        Ok(state) => state,
        Err(error) => return UnblockOutcome::Failed(error.phrase()),
    };
    match state::evaluate(&state, routes, false) {
        Update::Show(DisplayState::Blocked | DisplayState::ConnectingBlocked) => {}
        Update::Show(_) => return UnblockOutcome::NotBlocked,
        // An unknown answer: without blocking routes nothing to do, otherwise do not guess.
        Update::Keep | Update::Refresh if !routes => return UnblockOutcome::NotBlocked,
        Update::Keep | Update::Refresh => {
            return UnblockOutcome::Failed(Phrase::key(Key::ErrorInvalidResponse));
        }
    }
    if let Err(error) = service.unblock() {
        return UnblockOutcome::Failed(error.phrase());
    }
    let start = Instant::now();
    loop {
        if matches!(read_routes(), Ok(false)) {
            return UnblockOutcome::Freed;
        }
        if start.elapsed() >= wait.total {
            return UnblockOutcome::Failed(Phrase::raw(""));
        }
        thread::sleep(wait.step);
    }
}

/// 0 when the internet is free afterwards, 1 otherwise.
pub fn exit_code(outcome: &UnblockOutcome) -> u8 {
    match outcome {
        UnblockOutcome::Freed | UnblockOutcome::NotBlocked => 0,
        UnblockOutcome::Unreadable | UnblockOutcome::Failed(_) => 1,
    }
}

/// Whether the message goes to standard output (success) or standard error.
pub fn is_success(outcome: &UnblockOutcome) -> bool {
    exit_code(outcome) == 0
}

/// One sentence, and after a failure a second line with the emergency exit.
pub fn message(outcome: &UnblockOutcome, language: Language) -> String {
    let emergency = i18n::text(language, Key::CliEmergency);
    match outcome {
        UnblockOutcome::Freed => i18n::text(language, Key::CliFreed).to_owned(),
        UnblockOutcome::NotBlocked => i18n::text(language, Key::CliNotBlocked).to_owned(),
        UnblockOutcome::Unreadable => format!("{}\n{emergency}", i18n::text(language, Key::CliUnreadable)),
        UnblockOutcome::Failed(reason) => {
            let text = Phrase::fill(Key::CliFailed, [reason.clone()]).render(language);
            format!("{}\n{emergency}", text.trim_end())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    /// Test double of the service: a fixed state, records calls, and frees the routes after `/destroy`.
    struct Fake {
        state: Result<&'static str, Error>,
        destroy: Result<(), Error>,
        calls: RefCell<Vec<&'static str>>,
        routes: Cell<bool>,
        frees: bool,
    }

    impl Fake {
        fn new(code: &'static str, routes: bool) -> Self {
            Self {
                state: Ok(code),
                destroy: Ok(()),
                calls: RefCell::default(),
                routes: Cell::new(routes),
                frees: true,
            }
        }

        fn routes(&self) -> bool {
            self.routes.get()
        }
    }

    impl Service for Fake {
        fn state(&self) -> Result<ServiceState, Error> {
            self.calls.borrow_mut().push("state");
            self.state.clone().map(|code| ServiceState {
                code: code.to_owned(),
                ..ServiceState::default()
            })
        }

        fn unblock(&self) -> Result<(), Error> {
            self.calls.borrow_mut().push("destroy");
            if self.frees {
                self.routes.set(false);
            }
            self.destroy.clone()
        }
    }

    const FAST: Wait = Wait {
        total: Duration::from_millis(40),
        step: Duration::from_millis(5),
    };

    #[test]
    fn sperre_wird_aufgehoben() {
        for code in ["routed", "clean", "connecting"] {
            let fake = Fake::new(code, true);
            let outcome = unblock(&fake, || Ok(fake.routes()), FAST);
            assert_eq!(outcome, UnblockOutcome::Freed, "{code}");
            assert_eq!(*fake.calls.borrow(), ["state", "destroy"], "{code}");
            assert_eq!(exit_code(&outcome), 0);
        }
    }

    #[test]
    fn ohne_sperre_kein_aufruf() {
        for (code, routes) in [
            ("connected", true),
            ("connected", false),
            ("clean", false),
            ("routed", false),
        ] {
            let fake = Fake::new(code, routes);
            let outcome = unblock(&fake, || Ok(fake.routes()), FAST);
            assert_eq!(outcome, UnblockOutcome::NotBlocked, "{code} {routes}");
            assert_eq!(*fake.calls.borrow(), ["state"], "nur der Zustand: {code}");
        }
    }

    #[test]
    fn unlesbare_routen_ohne_jeden_aufruf() {
        let fake = Fake::new("routed", true);
        let outcome = unblock(&fake, || Err(io::ErrorKind::NotFound.into()), FAST);
        assert_eq!(outcome, UnblockOutcome::Unreadable);
        assert!(fake.calls.borrow().is_empty());
        assert_eq!(exit_code(&outcome), 1);
    }

    #[test]
    fn dienst_weg_oder_ohne_berechtigung() {
        for error in [Error::NotRunning, Error::PermissionDenied] {
            let fake = Fake {
                state: Err(error.clone()),
                ..Fake::new("routed", true)
            };
            let outcome = unblock(&fake, || Ok(fake.routes()), FAST);
            assert_eq!(outcome, UnblockOutcome::Failed(error.phrase()));
            assert_eq!(*fake.calls.borrow(), ["state"]);
        }
    }

    #[test]
    fn aufheben_scheitert_oder_sperre_bleibt() {
        let fake = Fake {
            destroy: Err(Error::Busy),
            frees: false,
            ..Fake::new("routed", true)
        };
        let outcome = unblock(&fake, || Ok(fake.routes()), FAST);
        assert_eq!(outcome, UnblockOutcome::Failed(Error::Busy.phrase()));
        let fake = Fake {
            frees: false,
            ..Fake::new("routed", true)
        };
        let start = Instant::now();
        let outcome = unblock(&fake, || Ok(fake.routes()), FAST);
        assert!(matches!(outcome, UnblockOutcome::Failed(_)));
        assert!(start.elapsed() >= FAST.total);
        assert_eq!(exit_code(&outcome), 1);
    }

    #[test]
    fn standardwartezeit_bleibt_unter_fuenf_sekunden() {
        let wait = Wait::default();
        assert_eq!(wait.total, Duration::from_secs(3));
        assert_eq!(wait.step, Duration::from_millis(250));
    }

    #[test]
    fn meldungen_in_beiden_sprachen() {
        assert_eq!(message(&UnblockOutcome::Freed, Language::En), "Internet unblocked.");
        assert_eq!(message(&UnblockOutcome::Freed, Language::De), "Internet freigegeben.");
        assert_eq!(
            message(&UnblockOutcome::NotBlocked, Language::En),
            "The internet is not blocked."
        );
        assert_eq!(
            message(&UnblockOutcome::NotBlocked, Language::De),
            "Das Internet ist nicht gesperrt."
        );
        for language in Language::ALL {
            for outcome in [
                UnblockOutcome::Unreadable,
                UnblockOutcome::Failed(Error::NotRunning.phrase()),
                UnblockOutcome::Failed(Phrase::raw("")),
            ] {
                let text = message(&outcome, language);
                assert!(text.ends_with("sudo systemctl stop celata-hide.me.service"), "{text}");
                assert_eq!(text.lines().count(), 2, "{text}");
                assert!(!is_success(&outcome));
            }
        }
        let text = message(&UnblockOutcome::Failed(Error::NotRunning.phrase()), Language::De);
        assert!(text.starts_with("Die Sperre ließ sich nicht aufheben. "), "{text}");
        assert!(is_success(&UnblockOutcome::Freed));
    }
}
