// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! Only one running window: the first start owns a name on the session bus; a second start asks it to bring its
//! window back and ends.
//!
//! The name offers exactly one method, `Activate()`, without arguments and without an answer. It only brings the
//! window back; it never connects, disconnects or blocks. If anything fails, Celata rather starts a second time
//! than not at all.

use std::time::Duration;

use zbus::blocking::Connection;
use zbus::fdo::RequestNameReply;

/// Name on the session bus.
pub const BUS_NAME: &str = "io.github.derhouwer.Celata";
/// Object that offers `Activate()`.
pub const PATH: &str = "/io/github/derhouwer/Celata";
/// Interface with the single method `Activate()`.
pub const INTERFACE: &str = "io.github.derhouwer.Celata1";
/// Longest wait for the session bus.
const TIMEOUT: Duration = Duration::from_secs(2);

/// Result of claiming the name.
pub enum Claim {
    /// This is the first start. Keep the connection open for the whole run: closing it gives the name up.
    Owner(Connection),
    /// Another Celata runs and brings its window back; end without a window.
    Other,
    /// No session bus, or the other start does not answer: start normally.
    NoBus,
}

/// What the bus answered, without the connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// The name is ours.
    Granted,
    /// Another process owns it.
    Taken,
    /// No answer or an error.
    Failed,
}

/// What follows from the answer of the bus and, if the name is taken, from calling `Activate()` at its owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Owner,
    Other,
    NoBus,
}

/// Decides from the answers. A taken name whose owner does not answer counts as no bus: two windows are better
/// than none.
pub fn outcome(answer: Answer, activated: bool) -> Outcome {
    match answer {
        Answer::Granted => Outcome::Owner,
        Answer::Taken if activated => Outcome::Other,
        Answer::Taken | Answer::Failed => Outcome::NoBus,
    }
}

/// The object behind [`PATH`]; it only calls `on_activate`.
struct Activator {
    on_activate: Box<dyn Fn() + Send + Sync>,
}

#[zbus::interface(name = "io.github.derhouwer.Celata1")]
impl Activator {
    /// Brings the window of Celata back.
    fn activate(&self) {
        (self.on_activate)();
    }
}

/// Reads the answer to `RequestName`. zbus reports a taken name as the error `NameTaken`, not as the answer
/// `Exists`.
fn answer_of(reply: &zbus::Result<RequestNameReply>) -> Answer {
    match reply {
        Ok(RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner) => Answer::Granted,
        Ok(RequestNameReply::Exists | RequestNameReply::InQueue) | Err(zbus::Error::NameTaken) => Answer::Taken,
        Err(_) => Answer::Failed,
    }
}

fn request(connection: &Connection) -> Answer {
    answer_of(&connection.request_name_with_flags(BUS_NAME, zbus::fdo::RequestNameFlags::DoNotQueue.into()))
}

fn activate_other(connection: &Connection) -> bool {
    connection
        .call_method(Some(BUS_NAME), PATH, Some(INTERFACE), "Activate", &())
        .is_ok()
}

/// Claims the name of Celata on the session bus; `on_activate` runs in a thread of zbus when a second start
/// calls `Activate()`.
pub fn claim(on_activate: impl Fn() + Send + Sync + 'static) -> Claim {
    let built = zbus::blocking::connection::Builder::session()
        .map(|builder| builder.method_timeout(TIMEOUT))
        .and_then(|builder| {
            builder.serve_at(
                PATH,
                Activator {
                    on_activate: Box::new(on_activate),
                },
            )
        })
        .and_then(zbus::blocking::connection::Builder::build);
    let Ok(connection) = built else {
        return Claim::NoBus;
    };
    let answer = request(&connection);
    let activated = answer == Answer::Taken && activate_other(&connection);
    match outcome(answer, activated) {
        Outcome::Owner => Claim::Owner(connection),
        Outcome::Other => Claim::Other,
        Outcome::NoBus => Claim::NoBus,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namen_auf_dem_sitzungsbus() {
        assert_eq!(BUS_NAME, "io.github.derhouwer.Celata");
        assert_eq!(PATH, "/io/github/derhouwer/Celata");
        assert_eq!(INTERFACE, "io.github.derhouwer.Celata1");
        assert!(zbus::names::WellKnownName::try_from(BUS_NAME).is_ok());
        assert!(zbus::zvariant::ObjectPath::try_from(PATH).is_ok());
        assert!(zbus::names::InterfaceName::try_from(INTERFACE).is_ok());
    }

    #[test]
    fn vergebener_name_ist_ein_fehler_von_zbus() {
        assert_eq!(answer_of(&Ok(RequestNameReply::PrimaryOwner)), Answer::Granted);
        assert_eq!(answer_of(&Ok(RequestNameReply::AlreadyOwner)), Answer::Granted);
        assert_eq!(answer_of(&Ok(RequestNameReply::Exists)), Answer::Taken);
        assert_eq!(answer_of(&Err(zbus::Error::NameTaken)), Answer::Taken);
        assert_eq!(answer_of(&Err(zbus::Error::InvalidReply)), Answer::Failed);
    }

    #[test]
    fn entscheidung_aus_den_antworten() {
        assert_eq!(outcome(Answer::Granted, false), Outcome::Owner);
        assert_eq!(outcome(Answer::Granted, true), Outcome::Owner);
        assert_eq!(outcome(Answer::Taken, true), Outcome::Other);
        // The owner does not answer: start normally rather than not at all.
        assert_eq!(outcome(Answer::Taken, false), Outcome::NoBus);
        assert_eq!(outcome(Answer::Failed, false), Outcome::NoBus);
        assert_eq!(outcome(Answer::Failed, true), Outcome::NoBus);
    }
}
