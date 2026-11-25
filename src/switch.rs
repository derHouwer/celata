// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! Switching to another server while connected, generalized to "reconnect with a target" for changes of
//! settings.
//!
//! Sequence: `/disconnect`, then `POST /configuration` with the new host, then `/connect`. `/disconnect`
//! leaves the block of the service in place, so between the old and the new connection the internet is
//! blocked instead of unprotected. Before that, Celata checks whether the block is ready and whether the
//! new name has an address. The sequence follows the source code of hide.me v0.9.14 and was confirmed
//! against the real service.

use std::io;

use crate::best::MAX_TRIES;
use crate::daemon::{Client, Error};
use crate::i18n::{Key, Phrase};
use crate::settings::{Class, Ledger, Settings, untested_notice};
use crate::state::{EXPECTED_VERSION, Notice, run_disconnect};
use crate::streaming::server_label;

/// Target of a reconnection: server and settings. Switching servers = other host, same settings; change
/// of settings = same host, other settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub host: String,
    pub settings: Settings,
}

impl Target {
    pub fn new(host: &str, settings: Settings) -> Self {
        Self {
            host: host.to_owned(),
            settings,
        }
    }
}

/// Part of a switch: to the new server, or back to the previous one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Forward,
    Back,
}

/// Result of a switch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SwitchResult {
    /// Connected to the new server.
    Switched,
    /// The new server failed; Celata is connected to the previous one again.
    Returned(Error),
    /// The new server and the way back both failed. The internet stays blocked.
    Stranded(Error),
    /// The block of the service is not ready or cannot be checked. The service was not called.
    Refused,
    /// The new name has no address. The service was not called.
    Unreachable,
    /// `/disconnect` was refused (HTTP 409); the connection is unchanged.
    NotStarted(Error),
    /// `/disconnect` failed with an unclear outcome. The next query shows the state.
    Unclear(Error),
    /// "Disconnect" was requested. Contains the result of `/destroy`.
    Stopped(Result<(), Error>),
    /// The service refused the new settings (HTTP 400) before anything was disconnected. The connection is
    /// unchanged.
    Rejected(String),
    /// Settings differ from the defaults, but the service reports an untested version. No call to the service
    /// besides `/version`.
    Untested(String),
}

/// If the service is not running or the permission is missing, the display explains the situation better
/// than a notice. After `systemctl stop`, for example, there is no block left that could be lifted.
fn service_gone(error: &Error) -> bool {
    matches!(error, Error::NotRunning | Error::PermissionDenied)
}

impl SwitchResult {
    /// The service tried the new name and it failed, or the name has no address. Only then can a name built
    /// by Celata (`servers::country_host`) count as unknown to hide.me.
    pub fn name_failed(&self) -> bool {
        // Deliberately without `_`: a new result is meant to cause a compile error here.
        match self {
            Self::Returned(_) | Self::Stranded(_) | Self::Unreachable => true,
            Self::Switched
            | Self::Refused
            | Self::NotStarted(_)
            | Self::Unclear(_)
            | Self::Stopped(_)
            | Self::Rejected(_)
            | Self::Untested(_) => false,
        }
    }

    /// Notice after the switch from `from` to `to` (hosts), or `None` if none is needed.
    pub fn notice(&self, from: &str, to: &str) -> Option<Notice> {
        let (from, to) = (server_label(from), server_label(to));
        let reason = |text: Phrase, error: &Error| Phrase::with_reason(text, error.phrase());
        let text = match self {
            Self::Switched | Self::Stopped(_) => return None,
            Self::Stranded(error) | Self::Unclear(error) if service_gone(error) => return None,
            Self::Returned(error) => reason(Phrase::fill(Key::NoticeSwitchReturned, [to, from]), error),
            Self::Stranded(error) => reason(Phrase::fill(Key::NoticeSwitchStranded, [from]), error),
            Self::Refused => Phrase::key(Key::NoticeSwitchRefused),
            Self::Unreachable => Phrase::fill(Key::NoticeSwitchUnreachable, [to, from]),
            Self::NotStarted(error) => reason(Phrase::key(Key::NoticeConnectNotStarted), error),
            Self::Unclear(error) => reason(Phrase::key(Key::NoticeSwitchUnclear), error),
            Self::Rejected(message) => Phrase::fill(Key::NoticeSwitchRejected, [Phrase::raw(message)]),
            Self::Untested(version) => untested_notice(version),
        };
        Some(Notice { text, command: None })
    }
}

/// Target of a switch through "Best location": checks at most [`MAX_TRIES`] hosts of the order one
/// after another with `resolves` and returns the first one with an address, and the hosts without an
/// address before it (they leave "Best location" for this session). A further host is never asked.
pub fn first_reachable(hosts: &[String], resolves: impl Fn(&str) -> bool) -> (Option<String>, Vec<String>) {
    let mut without_address = Vec::new();
    for host in hosts.iter().take(MAX_TRIES) {
        if resolves(host) {
            return (Some(host.clone()), without_address);
        }
        without_address.push(host.clone());
    }
    (None, without_address)
}

/// Checks the version of the service before settings that differ from the defaults are sent.
/// `Ok(None)`: fine; `Ok(Some(version))`: untested version.
pub fn untested_version(client: &Client, settings: &Settings) -> Result<Option<String>, Error> {
    if settings.is_default() {
        return Ok(None);
    }
    let version = client.version()?;
    Ok((version != EXPECTED_VERSION).then_some(version))
}

/// Sends `target` to the service, as the complete set or only the host, and records it.
pub fn configure(client: &Client, ledger: &mut Ledger, target: &Target) -> Result<(), Error> {
    let full = ledger.send_full(&target.settings);
    client.configure(&target.settings, &target.host, full)?;
    ledger.record_sent(&target.settings, full);
    Ok(())
}

/// Connects. A final state other than `connected` counts as a failure.
fn connect(client: &Client) -> Result<(), Error> {
    match client.connect()? {
        state if state.code == "connected" => Ok(()),
        _ => Err(Error::InvalidResponse),
    }
}

/// Sets `target` and connects.
fn connect_to(client: &Client, ledger: &mut Ledger, target: &Target) -> Result<(), Error> {
    configure(client, ledger, target)?;
    connect(client)
}

/// `/destroy` and then connect: rebuilds the routes with the settings sent last.
fn rebuild_to(client: &Client, ledger: &mut Ledger, target: &Target) -> Result<(), Error> {
    client.destroy()?;
    ledger.record_built(&target.settings);
    connect(client)
}

/// `/disconnect` at the beginning of a switch; `Err` is the result if it fails.
fn disconnect_first(client: &Client) -> Result<(), SwitchResult> {
    match client.disconnect() {
        Ok(state) if state.code == "routed" => Ok(()),
        Ok(_) => Err(SwitchResult::Unclear(Error::InvalidResponse)),
        Err(Error::Busy) => Err(SwitchResult::NotStarted(Error::Busy)),
        Err(error) => Err(SwitchResult::Unclear(error)),
    }
}

/// Result if sending the new settings fails before disconnecting: nothing has happened.
fn not_sent(error: Error) -> SwitchResult {
    match error {
        Error::Service(message) => SwitchResult::Rejected(message),
        other => SwitchResult::NotStarted(other),
    }
}

/// Switches from `from` to `to`: same default settings, other server.
pub fn run_switch(
    client: &Client,
    from: &str,
    to: &str,
    armed: impl FnOnce() -> io::Result<bool>,
    resolves: impl Fn(&str) -> bool,
    proceed: impl FnMut(Phase) -> bool,
) -> SwitchResult {
    let from = Target::new(from, Settings::default());
    let to = Target::new(to, Settings::default());
    run_reconnect(
        client,
        &mut Ledger::default(),
        &from,
        &to,
        Class::Reconnect,
        armed,
        resolves,
        proceed,
    )
}

/// Reconnects while connected, from `from` to `to`.
///
/// Class [`Class::Reconnect`]: block ready? → name resolvable? → settings → `/disconnect` → `/connect`.
/// Way back: settings of `from` → `/connect`. The block stays in place in between.
/// Class [`Class::Rebuild`]: name resolvable? → settings → `/destroy` → `/connect`. Way back: settings of
/// `from` → `/destroy` → `/connect`. The internet is unprotected for a moment.
///
/// The settings go to the service **before** disconnecting; if it refuses them, the operation ends with
/// [`SwitchResult::Rejected`] without having disconnected. Exception, switching servers (other host):
/// there the disconnect comes first, because on disconnecting the service signs off from the old server
/// with the *current* host name. A switch does not change the settings; they were accepted once already.
///
/// - `armed`: is the block ready? Only asked if the kill switch is turned on according to `from`. If the
///   user has turned it off, there is nothing that could protect in between.
/// - `resolves`: does the name have an address? Otherwise the service would silently take the previous
///   server.
/// - `proceed`: asked before every step at the service. `false` ("Disconnect" requested) leads to
///   `/destroy` and [`SwitchResult::Stopped`].
///
/// If the way back fails as well, the service stays as it is; `/destroy` never follows on its own.
#[allow(clippy::too_many_arguments)]
pub fn run_reconnect(
    client: &Client,
    ledger: &mut Ledger,
    from: &Target,
    to: &Target,
    class: Class,
    armed: impl FnOnce() -> io::Result<bool>,
    resolves: impl Fn(&str) -> bool,
    mut proceed: impl FnMut(Phase) -> bool,
) -> SwitchResult {
    match untested_version(client, &to.settings) {
        Ok(None) => {}
        Ok(Some(version)) => return SwitchResult::Untested(version),
        Err(error) => return SwitchResult::NotStarted(error),
    }
    if class == Class::Reconnect && from.settings.kill_switch && !matches!(armed(), Ok(true)) {
        return SwitchResult::Refused;
    }
    if !resolves(&to.host) {
        return SwitchResult::Unreachable;
    }
    if !proceed(Phase::Forward) {
        return SwitchResult::Stopped(run_disconnect(client));
    }
    let new_host = !from.host.eq_ignore_ascii_case(&to.host);
    let attempt = if class == Class::Reconnect && new_host {
        if let Err(result) = disconnect_first(client) {
            return result;
        }
        if !proceed(Phase::Forward) {
            return SwitchResult::Stopped(run_disconnect(client));
        }
        connect_to(client, ledger, to)
    } else {
        if let Err(error) = configure(client, ledger, to) {
            return not_sent(error);
        }
        if !proceed(Phase::Forward) {
            return SwitchResult::Stopped(run_disconnect(client));
        }
        match class {
            Class::Reconnect => {
                if let Err(result) = disconnect_first(client) {
                    return result;
                }
                if !proceed(Phase::Forward) {
                    return SwitchResult::Stopped(run_disconnect(client));
                }
                connect(client)
            }
            Class::Rebuild => match client.destroy() {
                Err(error) => return SwitchResult::Unclear(error),
                Ok(_) => {
                    ledger.record_built(&to.settings);
                    if !proceed(Phase::Forward) {
                        return SwitchResult::Stopped(run_disconnect(client));
                    }
                    connect(client)
                }
            },
        }
    };
    let failure = match attempt {
        Ok(()) => return SwitchResult::Switched,
        Err(error) => error,
    };
    // Way back to `from`. With "reconnect" the internet stays blocked until then.
    if !proceed(Phase::Back) {
        return SwitchResult::Stopped(run_disconnect(client));
    }
    let back = match class {
        Class::Reconnect => connect_to(client, ledger, from),
        Class::Rebuild => configure(client, ledger, from).and_then(|()| rebuild_to(client, ledger, from)),
    };
    match back {
        Ok(()) => SwitchResult::Returned(failure),
        Err(error) => SwitchResult::Stranded(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn hosts(count: usize) -> Vec<String> {
        (1..=count).map(|index| format!("h{index}.hideservers.net")).collect()
    }

    /// Runs `first_reachable` with a check that answers yes only for `reachable` and records every question.
    fn ask(hosts: &[String], reachable: &[&str]) -> ((Option<String>, Vec<String>), Vec<String>) {
        let asked = RefCell::new(Vec::new());
        let result = first_reachable(hosts, |host| {
            asked.borrow_mut().push(host.to_owned());
            reachable.contains(&host)
        });
        (result, asked.into_inner())
    }

    #[test]
    fn erster_erreichbarer_kandidat() {
        let list = hosts(4);
        let ((found, missing), asked) = ask(&list, &["h1.hideservers.net", "h2.hideservers.net"]);
        assert_eq!(found.as_deref(), Some("h1.hideservers.net"));
        assert!(missing.is_empty());
        assert_eq!(asked.len(), 1);
        let ((found, missing), asked) = ask(&list, &["h2.hideservers.net"]);
        assert_eq!(found.as_deref(), Some("h2.hideservers.net"));
        assert_eq!(missing, ["h1.hideservers.net"]);
        assert_eq!(asked.len(), 2);
    }

    #[test]
    fn hoechstens_drei_namen() {
        let list = hosts(4);
        // The fourth would have an address, but is never asked.
        let ((found, missing), asked) = ask(&list, &["h4.hideservers.net"]);
        assert_eq!(found, None);
        assert_eq!(missing, hosts(3));
        assert_eq!(asked, hosts(3));
        let ((found, missing), asked) = ask(&[], &[]);
        assert_eq!(found, None);
        assert!(missing.is_empty() && asked.is_empty());
    }

    /// Feature 012: only results in which the new name was tried or had no address count as a failed name.
    #[test]
    fn nur_versuchte_namen_gelten_als_gescheitert() {
        for result in [
            SwitchResult::Returned(Error::Timeout),
            SwitchResult::Stranded(Error::Timeout),
            SwitchResult::Unreachable,
        ] {
            assert!(result.name_failed(), "{result:?}");
        }
        for result in [
            SwitchResult::Switched,
            SwitchResult::Refused,
            SwitchResult::NotStarted(Error::Timeout),
            SwitchResult::Unclear(Error::Timeout),
            SwitchResult::Stopped(Ok(())),
            SwitchResult::Rejected("x".to_owned()),
            SwitchResult::Untested("0.9.15".to_owned()),
        ] {
            assert!(!result.name_failed(), "{result:?}");
        }
    }
}
