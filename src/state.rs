// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! The states Celata can show, and how they follow from what the service reports.

use std::time::Instant;

use crate::daemon::{Client, Error, Flow, Observation, ServiceState};
use crate::i18n::{Key, Phrase};
use crate::look::ExternalIp;
use crate::servers::is_valid_host;
use crate::settings::{Ledger, Settings, untested_notice};
use crate::switch::{self, Phase, SwitchResult, Target, untested_version};
use crate::unit::{Before, GroupCheck, Interactive, Permit, Progress, StartFailure};

/// What the window shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisplayState {
    NotConnected,
    Connecting,
    ConnectingBlocked,
    Connected {
        server: Option<String>,
    },
    Disconnecting,
    /// A switch started by Celata is running. `target` is the host being connected to; `back` applies on the
    /// way back to the previous server. The internet is blocked on purpose for a moment.
    Switching {
        target: String,
        back: bool,
    },
    /// Celata reconnects after a change of settings; `back` on the way back to the previous settings.
    Reapplying {
        back: bool,
    },
    Blocked,
    ServiceUnreachable,
    ServiceUnreachableBlocked,
    NoPermission,
    /// Celata started the service (or found it starting) and waits for its first answer. `blocked`: the
    /// blocking routes of the kill switch still stand.
    ServiceStarting {
        blocked: bool,
    },
}

/// Result of evaluating a state of the service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Update {
    /// Show this state.
    Show(DisplayState),
    /// A mere notice: the display stays as it is.
    Keep,
    /// Unknown code: query `/state` again.
    Refresh,
}

/// Derives the display from the state of the service and the routes.
///
/// `own_connect_from_free` applies when Celata started the running connection attempt from the free state.
/// The blocking routes then exist only because of this attempt; the internet was free before.
pub fn evaluate(state: &ServiceState, blocked_routes: bool, own_connect_from_free: bool) -> Update {
    match state.code.as_str() {
        "connected" => Update::Show(DisplayState::Connected {
            server: state.host.as_deref().and_then(shown_host),
        }),
        "connecting" | "dns lookup" => Update::Show(if blocked_routes && !own_connect_from_free {
            DisplayState::ConnectingBlocked
        } else {
            DisplayState::Connecting
        }),
        "disconnecting" => Update::Show(DisplayState::Disconnecting),
        "clean" | "routed" | "dpd timeout" => Update::Show(if blocked_routes {
            DisplayState::Blocked
        } else {
            DisplayState::NotConnected
        }),
        "configuration get" | "configuration set" | "logs dumped" | "token update" | "token update done"
        | "external ips" => Update::Keep,
        _ => Update::Refresh,
    }
}

/// Longest server name that is shown (the maximum length of a DNS name).
const MAX_HOST: usize = 253;

/// Server name from the answer of the service for the display: without control characters, limited;
/// empty counts as unknown.
fn shown_host(host: &str) -> Option<String> {
    Some(crate::text::printable(host, MAX_HOST)).filter(|host| !host.is_empty())
}

/// Derives the display from a connection error. If it is unclear whether the service runs, the routes count.
pub fn evaluate_error(error: &Error, blocked_routes: bool) -> DisplayState {
    match error {
        Error::PermissionDenied => DisplayState::NoPermission,
        _ if blocked_routes => DisplayState::ServiceUnreachableBlocked,
        _ => DisplayState::ServiceUnreachable,
    }
}

/// Version of the service Celata was tested with.
pub const EXPECTED_VERSION: &str = "0.9.14";
/// Name of the service unit in systemd. The only place; the commands below must contain it literally
/// (test `befehle_nennen_den_einen_dienstnamen`).
pub const SERVICE_UNIT: &str = "celata-hide.me.service";
/// Starts the service by hand. Celata starts the service through systemd itself but never runs this command; it
/// stays the fallback in the display.
pub const START_COMMAND: &str = "sudo systemctl start celata-hide.me.service";
/// Restarts the service when it runs but does not answer correctly.
pub const RESTART_COMMAND: &str = "sudo systemctl restart celata-hide.me.service";
/// Emergency exit: stops the service and thereby lifts the block as well.
pub const STOP_COMMAND: &str = "sudo systemctl stop celata-hide.me.service";
/// Shows why the service does not start and its last messages; Celata never reads the log itself.
pub const STATUS_COMMAND: &str = "systemctl status celata-hide.me.service";
/// Adds the own user to the group (same command as in the documentation).
pub const GROUP_COMMAND: &str = "sudo usermod -aG celata \"$USER\"";

/// Text for a state: what is going on and what to do next. It is translated when shown; for "Switching
/// to …" and "Back to …" the title contains the placeholder `{0}` for the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub title: Key,
    pub text: Key,
    /// Command for the terminal, if the next step is there.
    pub command: Option<&'static str>,
    /// The state means a blocked internet connection and is clearly highlighted.
    pub warning: bool,
}

impl DisplayState {
    /// Does the service answer in this state?
    pub fn service_reachable(&self) -> bool {
        !matches!(
            self,
            Self::ServiceUnreachable
                | Self::ServiceUnreachableBlocked
                | Self::NoPermission
                | Self::ServiceStarting { .. }
        )
    }

    pub fn message(&self) -> Message {
        let plain = |title, text| Message {
            title,
            text,
            command: None,
            warning: false,
        };
        match self {
            Self::NotConnected => plain(Key::StateNotConnectedTitle, Key::StateNotConnectedText),
            Self::Connecting => plain(Key::StateConnectingTitle, Key::StateConnectingText),
            Self::ConnectingBlocked => Message {
                warning: true,
                ..plain(Key::StateConnectingBlockedTitle, Key::StateConnectingBlockedText)
            },
            Self::Connected { .. } => plain(Key::StateConnectedTitle, Key::StateConnectedText),
            Self::Disconnecting => plain(Key::StateDisconnectingTitle, Key::StateDisconnectingText),
            // `main.rs` fills in the server for `{0}` in the title; the label lives there (`server_label`).
            Self::Switching { back: false, .. } => plain(Key::StateSwitchingTitle, Key::StateSwitchingText),
            Self::Switching { back: true, .. } => plain(Key::StateReturningTitle, Key::StateReturningText),
            Self::Reapplying { back: false } => plain(Key::StateReapplyingTitle, Key::StateReapplyingText),
            Self::Reapplying { back: true } => plain(Key::StateReapplyingBackTitle, Key::StateReapplyingBackText),
            Self::Blocked => Message {
                warning: true,
                ..plain(Key::StateBlockedTitle, Key::StateBlockedText)
            },
            Self::ServiceUnreachable => Message {
                command: Some(START_COMMAND),
                ..plain(Key::StateServiceUnreachableTitle, Key::StateServiceUnreachableText)
            },
            Self::ServiceUnreachableBlocked => Message {
                command: Some(STOP_COMMAND),
                warning: true,
                ..plain(
                    Key::StateServiceUnreachableBlockedTitle,
                    Key::StateServiceUnreachableBlockedText,
                )
            },
            Self::NoPermission => Message {
                command: Some(GROUP_COMMAND),
                ..plain(Key::StateNoPermissionTitle, Key::StateNoPermissionText)
            },
            Self::ServiceStarting { blocked: false } => {
                plain(Key::StateServiceStartingTitle, Key::StateServiceStartingText)
            }
            Self::ServiceStarting { blocked: true } => Message {
                warning: true,
                ..plain(
                    Key::StateServiceStartingBlockedTitle,
                    Key::StateServiceStartingBlockedText,
                )
            },
        }
    }
}

/// Result of a connection attempt started by Celata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectResult {
    Connected,
    /// The attempt did not start at all; nothing has changed.
    NotStarted(Error),
    /// The service refused the settings (HTTP 400); no `/connect`.
    Rejected(String),
    /// Settings differ from the defaults, but the service reports this untested version; no call.
    Untested(String),
    /// Failed; the internet was free before and was released again with `/destroy`.
    FailedReleased(Error),
    /// Failed; the internet was blocked before, and the block stays.
    FailedBlocked(Error),
    /// Failed, and releasing with `/destroy` failed as well.
    ReleaseFailed(Error),
}

impl ConnectResult {
    /// The service tried the name and it failed. Only then can a name built by Celata
    /// (`servers::country_host`) count as unknown to hide.me.
    pub fn name_failed(&self) -> bool {
        // Deliberately without `_`: a new result is meant to cause a compile error here.
        match self {
            Self::FailedReleased(_) | Self::FailedBlocked(_) | Self::ReleaseFailed(_) => true,
            Self::Connected | Self::NotStarted(_) | Self::Rejected(_) | Self::Untested(_) => false,
        }
    }
}

/// Connects to `target`. `was_free` is the remembered initial state: only if the internet was free before
/// is it released again with `/destroy` after a failure.
///
/// Before that, the settings go to the service; settings that differ from the defaults only to version
/// [`EXPECTED_VERSION`]. `rebuild` (only from "Internet blocked"): the routes still stand with other values
/// of the class "rebuild", so after the accepted `POST /configuration` comes `/destroy` first and then
/// `/connect`. The internet is free for a moment; the caller warns beforehand.
pub fn run_connect(
    client: &Client,
    ledger: &mut Ledger,
    target: &Target,
    was_free: bool,
    rebuild: bool,
) -> ConnectResult {
    match untested_version(client, &target.settings) {
        Ok(None) => {}
        Ok(Some(version)) => return ConnectResult::Untested(version),
        Err(error) => return ConnectResult::NotStarted(error),
    }
    match switch::configure(client, ledger, target) {
        Ok(()) => {}
        Err(Error::Service(message)) => return ConnectResult::Rejected(message),
        Err(error) => return ConnectResult::NotStarted(error),
    }
    let rebuild = rebuild && !was_free;
    if rebuild && let Err(error) = client.destroy() {
        return ConnectResult::NotStarted(error);
    }
    let attempt = client.connect();
    // From `clean`, `/connect` creates the routes anew with the values just sent, even if the connection
    // fails afterwards.
    if (was_free || rebuild) && attempt != Err(Error::Busy) {
        ledger.record_built(&target.settings);
    }
    let error = match attempt {
        Ok(state) if state.code == "connected" => return ConnectResult::Connected,
        // HTTP 409: another operation is running; this attempt has not started.
        Err(Error::Busy) => return ConnectResult::NotStarted(Error::Busy),
        Ok(_) => Error::InvalidResponse,
        Err(error) => error,
    };
    if !was_free {
        return ConnectResult::FailedBlocked(error);
    }
    match client.destroy() {
        Ok(_) => ConnectResult::FailedReleased(error),
        Err(_) => ConnectResult::ReleaseFailed(error),
    }
}

/// Result of "Block internet".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteResult {
    /// The service reports `routed`; the block stands.
    Routed,
    /// Settings differ from the defaults, but the version of the service is untested; no call besides `/version`.
    Untested(String),
    /// The service refused the settings (HTTP 400); no `/route`.
    Rejected(String),
    /// `/route` or sending failed.
    Failed(Error),
    /// The kill switch is turned off in the settings; no call to the service.
    Disabled,
}

/// Blocks the internet without connecting (`GET /route`). Before that, the settings go to the service;
/// otherwise it would build the routes with old values. If all settings are at their defaults and nothing
/// else was sent in this run, nothing is sent. The set goes out without a host, because no server is
/// chosen when blocking.
pub fn run_route(client: &Client, ledger: &mut Ledger, settings: &Settings) -> RouteResult {
    if !settings.kill_switch {
        return RouteResult::Disabled;
    }
    match untested_version(client, settings) {
        Ok(None) => {}
        Ok(Some(version)) => return RouteResult::Untested(version),
        Err(error) => return RouteResult::Failed(error),
    }
    if ledger.send_full(settings) {
        match client.configure_without_host(settings) {
            Ok(()) => ledger.record_sent(settings, true),
            Err(Error::Service(message)) => return RouteResult::Rejected(message),
            Err(error) => return RouteResult::Failed(error),
        }
    }
    match client.route() {
        Ok(state) if state.code == "routed" => {
            ledger.record_built(settings);
            RouteResult::Routed
        }
        Ok(_) => RouteResult::Failed(Error::InvalidResponse),
        Err(error) => RouteResult::Failed(error),
    }
}

/// Explanation shown when "Block internet" is greyed out.
pub const ROUTE_HINT: Key = Key::RouteHint;

/// Disconnects and lifts a block. It always uses `/destroy`, never `/disconnect`, because that would
/// leave the internet blocked.
pub fn run_disconnect(client: &Client) -> Result<(), Error> {
    client.destroy().map(|_| ())
}

/// Lifts the block, with `/destroy` as well. [`Monitor::confirm_unblock`] must have agreed before.
pub fn run_unblock(client: &Client) -> Result<(), Error> {
    run_disconnect(client)
}

/// Content of the question asked when the window is closed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloseDialog {
    pub text: Key,
    /// Offer "Disconnect and close".
    pub offers_disconnect: bool,
    /// Label for "close without changing anything".
    pub keep_label: Key,
    /// Emergency exit, if the block cannot be lifted from within Celata.
    pub command: Option<&'static str>,
}

/// What happens when the window is closed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloseDecision {
    /// Close without asking.
    Close,
    /// Ask first; "Cancel" is always offered.
    Ask(CloseDialog),
}

/// Decides whether to ask on closing: while connected or connecting, and as a notice beforehand while the
/// internet is blocked. In the disconnected state the window closes at once.
pub fn close_decision(display: Option<&DisplayState>) -> CloseDecision {
    let dialog = |text, offers_disconnect, keep_label, command| {
        CloseDecision::Ask(CloseDialog {
            text,
            offers_disconnect,
            keep_label,
            command,
        })
    };
    match display {
        Some(DisplayState::Connected { .. }) => dialog(Key::CloseConnectedText, true, Key::CloseKeepConnected, None),
        // A switch asks like a connection attempt.
        Some(
            DisplayState::Connecting
            | DisplayState::Disconnecting
            | DisplayState::Switching { .. }
            | DisplayState::Reapplying { .. },
        ) => dialog(Key::CloseBusyText, true, Key::CloseKeepConnected, None),
        Some(DisplayState::ConnectingBlocked | DisplayState::Blocked) => {
            dialog(Key::CloseBlockedText, true, Key::CloseKeepBlocked, None)
        }
        Some(DisplayState::ServiceUnreachableBlocked | DisplayState::ServiceStarting { blocked: true }) => dialog(
            Key::CloseOfflineBlockedText,
            false,
            Key::CloseAnyway,
            Some(STOP_COMMAND),
        ),
        None
        | Some(
            DisplayState::NotConnected
            | DisplayState::ServiceUnreachable
            | DisplayState::NoPermission
            | DisplayState::ServiceStarting { blocked: false },
        ) => CloseDecision::Close,
    }
}

/// Additional notice after an action, for example after a failed connection attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub text: Phrase,
    pub command: Option<&'static str>,
}

impl ConnectResult {
    fn notice(&self) -> Option<Notice> {
        let text = |text: Key, error: &Error| Phrase::with_reason(Phrase::key(text), error.phrase());
        let (text, command) = match self {
            Self::Connected => return None,
            Self::NotStarted(error) => (text(Key::NoticeConnectNotStarted, error), None),
            Self::Rejected(message) => (Phrase::fill(Key::NoticeConnectRejected, [Phrase::raw(message)]), None),
            Self::Untested(version) => (untested_notice(version), None),
            Self::FailedReleased(error) => (text(Key::NoticeConnectFailedReleased, error), None),
            Self::FailedBlocked(error) => (text(Key::NoticeConnectFailedBlocked, error), None),
            Self::ReleaseFailed(error) => (text(Key::NoticeConnectReleaseFailed, error), Some(STOP_COMMAND)),
        };
        Some(Notice { text, command })
    }
}

/// What the window shows right now, ready for the interface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusView {
    pub message: Message,
    /// Server name while "Connected".
    pub server: Option<String>,
    /// Additional detail on the reason, if the service is unreachable in an unexpected way.
    pub detail: Option<Phrase>,
    /// Notice if the service reports another version than the tested one.
    pub version_warning: Option<Phrase>,
    /// Notice after an action; its command takes precedence over that of the message.
    pub notice: Option<Notice>,
    /// "Connect" is possible: from "Not connected" or "Internet blocked", and no attempt is running.
    pub can_connect: bool,
    /// A connection attempt started by Celata is running.
    pub connecting: bool,
    /// "Disconnect" is possible: while "Connected" with no other action running, or during a switch.
    pub can_disconnect: bool,
    /// Switching to another server is possible: "Connected" with a known server, no action running.
    pub can_switch: bool,
    /// Host of the connected server, otherwise empty. The interface uses it to grey out "Connect".
    pub connected_host: String,
    /// A switch is running.
    pub switching: bool,
    /// Target of the running switch for the title "Switching to …" or "Back to …".
    pub switch_target: Option<String>,
    /// Target of the own connection attempt for the title "Connecting to …" (feature 013).
    pub connect_target: Option<String>,
    /// Offer "Unblock internet".
    pub can_unblock: bool,
    /// Show the warning before lifting the block ([`UNBLOCK_WARNING`]).
    pub unblock_asked: bool,
    /// State read last while "Connected"; source of the connection details.
    pub service: Option<ServiceState>,
    /// State of the public address.
    pub external_ip: ExternalIp,
    /// "Not connected" without an operation: show "Block internet" (possible only with the kill switch on).
    pub route_ready: bool,
    /// The operation "Block internet" is running (between the click and the result of `/route`).
    pub routing: bool,
    /// The service is unreachable, and connecting starts it first: no reason against a start without a password
    /// is known, and no start is running.
    pub start_on_connect: bool,
    /// The button "Start service" (with a password prompt).
    pub start_button: StartButton,
    /// Show the section "Fix permanently" with the command for the rule.
    pub fix_rule: bool,
    /// Show the link to the setup guide.
    pub guide_link: bool,
}

/// The button "Start service" in the status card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StartButton {
    #[default]
    Hidden,
    Ready,
    /// The password prompt is open: shown, but disabled.
    Busy,
}

/// Processes the reports of the observation and remembers what is shown.
#[derive(Debug, Clone, Default)]
pub struct Monitor {
    display: Option<DisplayState>,
    detail: Option<Phrase>,
    version_warning: Option<Phrase>,
    notice: Option<Notice>,
    /// Counts every notice set, also one with the same text as before, so that a message can be told apart
    /// from the one shown at an earlier moment.
    notice_count: u64,
    /// Running action of Celata itself, so that no second one starts at the same time.
    action: Option<Action>,
    /// The warning before lifting the block is being shown.
    unblock_asked: bool,
    /// State read last with the code `connected`.
    service: Option<ServiceState>,
    external_ip: ExternalIp,
    /// Start of the service by Celata.
    start: StartTracker,
}

/// What Celata knows and does about starting the service, for this session only; nothing of it is stored.
#[derive(Debug, Clone, Default)]
struct StartTracker {
    phase: StartPhase,
    permit: Permit,
    /// Reason of the last failed start; it stays until the service answers.
    failure: Option<StartFailure>,
    /// Target of the wish to connect that started the service (a host or `best::BEST`).
    then: Option<String>,
    /// Target released by the first answer of the service, waiting for `take_connect_after_start`.
    connect_due: Option<String>,
    /// Bring a hidden window back once, because a start ended with a reason.
    reveal: bool,
    /// Group of this process, read before the last start.
    group: GroupCheck,
    /// The check at program start was requested already.
    checked: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum StartPhase {
    #[default]
    Idle,
    /// The state of the unit is being read before a start. `starting`: "Starting service …" is shown meanwhile
    /// (a wish to connect); at program start "Checking status …" stays.
    Checking { starting: bool },
    /// The password prompt after the button is open.
    Asking,
    /// The start was accepted or found running (`own`: accepted from Celata); waiting for the first answer.
    /// Until `unit_ready` errors of the socket mean "still starting".
    Running {
        since: Instant,
        unit_ready: bool,
        own: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Action {
    /// Own connection attempt; `was_free` is the remembered initial state, `target` the host.
    Connect { was_free: bool, target: String },
    /// Disconnecting; `closing` applies to "Disconnect and close".
    Disconnect { closing: bool },
    /// Lifting the block after confirmation.
    Unblock,
    /// "Block internet" without a connection.
    Route,
    /// Switch from `from` to `to`. `stop` is a wish from "Disconnect" (`false`) or "Disconnect and close"
    /// (`true`), which the switch thread carries out after the running step. `reapply`: reconnecting after a
    /// change of settings, same host. `called`: the switch thread has reached its first step at the service
    /// (`switch_proceed`); from then on the target is fixed.
    Switch {
        from: String,
        to: String,
        phase: Phase,
        stop: Option<bool>,
        reapply: bool,
        called: bool,
    },
}

/// What a click on "Connect" or a double click starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Start {
    /// Connecting from "Not connected" or "Internet blocked"; `was_free` is the initial state.
    Connect { was_free: bool },
    /// Switch from the connected server `from`.
    Switch { from: String },
}

/// Whether to reconnect now after a change of settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReapplyGate {
    /// "Connected" without a running operation: reconnect now with this host.
    Now(String),
    /// A switch, a connection attempt or a disconnect is running: the bundle of changes waits.
    Wait,
    /// No connection: the change applies from the next connection on; no call to the service.
    Never,
}

/// Message after a successful reconnection.
pub const REAPPLIED: Key = Key::NoticeReapplied;

/// Notice after reconnecting; `None` only if "Disconnect" took over or the display says more.
pub fn reapply_notice(result: &SwitchResult) -> Option<Phrase> {
    let later = || Phrase::key(Key::NoticeReapplyLater);
    Some(match result {
        SwitchResult::Switched => Phrase::key(REAPPLIED),
        SwitchResult::Stopped(_) => return None,
        SwitchResult::Returned(error) => Phrase::fill(Key::NoticeReapplyReturned, [error.phrase()]),
        SwitchResult::Rejected(message) => Phrase::fill(Key::NoticeReapplyRejected, [Phrase::raw(message)]),
        SwitchResult::Untested(version) => untested_notice(version),
        SwitchResult::Refused => Phrase::fill(Key::NoticeReapplyRefused, [later()]),
        SwitchResult::Unreachable => Phrase::fill(Key::NoticeReapplyUnreachable, [later()]),
        SwitchResult::NotStarted(error) => {
            Phrase::with_reason(Phrase::fill(Key::NoticeReapplyNotStarted, [later()]), error.phrase())
        }
        SwitchResult::Stranded(error) => Phrase::with_reason(Phrase::key(Key::NoticeReapplyStranded), error.phrase()),
        SwitchResult::Unclear(error) => Phrase::with_reason(Phrase::key(Key::NoticeReapplyUnclear), error.phrase()),
    })
}

/// What "Disconnect" triggers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisconnectStart {
    /// `/destroy` at once, in a thread of its own.
    Now,
    /// A switch is running: its thread carries out `/destroy` after the running step.
    Deferred,
    /// Not possible right now.
    No,
}

/// Warning before lifting the block.
pub const UNBLOCK_WARNING: Key = Key::UnblockWarning;

/// Display during a switch: intermediate states of the service show "Switching to …" or "Back to …", never
/// a block warning. "Connected" only with the host of the target. If "Disconnect" is requested,
/// "Disconnecting …".
fn during_switch(display: DisplayState, target: &str, phase: Phase, stopping: bool, reapply: bool) -> DisplayState {
    let switching = || {
        if stopping {
            DisplayState::Disconnecting
        } else if reapply {
            DisplayState::Reapplying {
                back: phase == Phase::Back,
            }
        } else {
            DisplayState::Switching {
                target: target.to_owned(),
                back: phase == Phase::Back,
            }
        }
    };
    // When reconnecting, the initial state is already "Connected" with the same host: only the result
    // decides.
    if reapply
        && !matches!(
            display,
            DisplayState::ServiceUnreachable
                | DisplayState::ServiceUnreachableBlocked
                | DisplayState::NoPermission
                | DisplayState::ServiceStarting { .. }
        )
    {
        return switching();
    }
    match display {
        DisplayState::Connected { server: Some(server) } if server.eq_ignore_ascii_case(target) => {
            DisplayState::Connected { server: Some(server) }
        }
        DisplayState::NotConnected
        | DisplayState::Blocked
        | DisplayState::Connecting
        | DisplayState::ConnectingBlocked
        | DisplayState::Disconnecting
        | DisplayState::Connected { .. } => switching(),
        other => other,
    }
}

impl Monitor {
    pub fn new() -> Self {
        Self::default()
    }

    /// What happens when the window is closed.
    pub fn close_decision(&self) -> CloseDecision {
        close_decision(self.display.as_ref())
    }

    /// The current display state, or `None` as long as nothing is known yet.
    pub fn display(&self) -> Option<&DisplayState> {
        self.display.as_ref()
    }

    /// Number of notices set so far; it grows with every new notice, even with the same text.
    pub fn notice_count(&self) -> u64 {
        self.notice_count
    }

    fn count_notice(&mut self) {
        if self.notice.is_some() {
            self.notice_count = self.notice_count.wrapping_add(1);
        }
    }

    fn own_connect_from_free(&self) -> bool {
        matches!(self.action, Some(Action::Connect { was_free: true, .. }))
    }

    /// Host of the connected server, if "Connected" is shown and the host is known.
    fn connected_server(&self) -> Option<&str> {
        match &self.display {
            Some(DisplayState::Connected { server: Some(server) }) => Some(server),
            _ => None,
        }
    }

    /// Starts a connection attempt of Celata to `host` or, while connected, a switch to it.
    ///
    /// From "Not connected" or "Internet blocked" it connects and remembers the initial state. From
    /// "Connected" it switches, but only to a valid other host. `None` if nothing may start right now, for
    /// example because an operation is already running (double click).
    pub fn begin_connect(&mut self, host: &str) -> Option<Start> {
        if self.view().can_connect {
            let was_free = self.display == Some(DisplayState::NotConnected);
            self.action = Some(Action::Connect {
                was_free,
                target: host.to_owned(),
            });
            self.notice = None;
            let display = if was_free {
                DisplayState::Connecting
            } else {
                DisplayState::ConnectingBlocked
            };
            self.show(display, None);
            return Some(Start::Connect { was_free });
        }
        if self.action.is_some() || !is_valid_host(host) {
            return None;
        }
        let from = self.connected_server()?.to_owned();
        if host.eq_ignore_ascii_case(&from) {
            return None;
        }
        self.action = Some(Action::Switch {
            from: from.clone(),
            to: host.to_owned(),
            phase: Phase::Forward,
            stop: None,
            reapply: false,
            called: false,
        });
        self.notice = None;
        self.show(
            DisplayState::Switching {
                target: host.to_owned(),
                back: false,
            },
            None,
        );
        Some(Start::Switch { from })
    }

    /// Replaces the target of a running switch, before the switch thread has called the service
    /// (`switch_proceed`): "Best location" found that the first candidate has no address. Only for a
    /// switch to another server, not while reconnecting, not after "Disconnect", only to a valid host other
    /// than the connected one. Returns whether the target was replaced.
    pub fn retarget(&mut self, host: &str) -> bool {
        let Some(Action::Switch {
            from,
            to,
            phase: Phase::Forward,
            stop: None,
            reapply: false,
            called: false,
        }) = &mut self.action
        else {
            return false;
        };
        if !is_valid_host(host) || host.eq_ignore_ascii_case(from) {
            return false;
        }
        host.clone_into(to);
        self.show(
            DisplayState::Switching {
                target: host.to_owned(),
                back: false,
            },
            None,
        );
        true
    }

    /// Ends a switch before the switch thread called the service and stays connected to the previous server
    /// with `notice`: "Best location" found during the check of names that the connected server is the best
    /// reachable one after all. Not after the first step, not after "Disconnect" was requested, not
    /// while reconnecting. Returns whether the switch ended.
    pub fn abandon_switch(&mut self, notice: Phrase) -> bool {
        let Some(Action::Switch {
            from,
            phase: Phase::Forward,
            stop: None,
            reapply: false,
            called: false,
            ..
        }) = &self.action
        else {
            return false;
        };
        let from = from.clone();
        self.action = None;
        self.notice = Some(Notice {
            text: notice,
            command: None,
        });
        self.count_notice();
        self.show(DisplayState::Connected { server: Some(from) }, None);
        true
    }

    /// Shows a notice without starting an operation, for example "Already connected to the best location."
    /// Only while no operation is running; the notice disappears with the next one. Returns whether it is
    /// shown.
    pub fn inform(&mut self, text: Phrase) -> bool {
        if self.action.is_some() {
            return false;
        }
        self.notice = Some(Notice { text, command: None });
        self.count_notice();
        true
    }

    /// "Block internet" is possible: "Not connected", no operation, kill switch on in the settings.
    pub fn can_route(&self, kill_switch: bool) -> bool {
        kill_switch && self.action.is_none() && self.display == Some(DisplayState::NotConnected)
    }

    /// Starts "Block internet", without asking. Returns `false` if it is not possible right now; an outdated
    /// click then does nothing.
    pub fn begin_route(&mut self, kill_switch: bool) -> bool {
        if !self.can_route(kill_switch) {
            return false;
        }
        self.action = Some(Action::Route);
        self.notice = None;
        true
    }

    /// Ends "Block internet". The new state comes from a query of `/state` afterwards.
    pub fn finish_route(&mut self, result: &RouteResult) {
        if self.action != Some(Action::Route) {
            return;
        }
        self.action = None;
        let text = match result {
            RouteResult::Routed => return,
            RouteResult::Untested(version) => untested_notice(version),
            RouteResult::Rejected(message) => Phrase::fill(Key::NoticeRouteRejected, [Phrase::raw(message)]),
            RouteResult::Failed(error) => Phrase::with_reason(Phrase::key(Key::NoticeRouteFailed), error.phrase()),
            RouteResult::Disabled => Phrase::key(Key::RouteHint),
        };
        self.notice = Some(Notice { text, command: None });
        self.count_notice();
    }

    /// May Celata reconnect now after a change of settings?
    pub fn reapply_gate(&self) -> ReapplyGate {
        if self.action.is_some() {
            return ReapplyGate::Wait;
        }
        match self.connected_server() {
            Some(host) => ReapplyGate::Now(host.to_owned()),
            None => ReapplyGate::Never,
        }
    }

    /// Starts reconnecting with the connected host. Display as for a switch, but with the title "Applying
    /// settings …". `None` if reconnecting is not allowed right now.
    pub fn begin_reapply(&mut self) -> Option<String> {
        let ReapplyGate::Now(host) = self.reapply_gate() else {
            return None;
        };
        self.action = Some(Action::Switch {
            from: host.clone(),
            to: host.clone(),
            phase: Phase::Forward,
            stop: None,
            reapply: true,
            called: false,
        });
        self.notice = None;
        self.show(DisplayState::Reapplying { back: false }, None);
        Some(host)
    }

    /// Asked by the switch thread before every step at the service. Returns `false` if it should stop: then
    /// "Disconnect" was requested, and the action becomes `Disconnect`, so that [`Monitor::finish_disconnect`]
    /// finishes as usual. With `Phase::Back` the display shows "Back to …".
    pub fn switch_proceed(&mut self, phase: Phase) -> bool {
        let Some(Action::Switch {
            from,
            phase: current,
            stop,
            reapply,
            called,
            ..
        }) = &mut self.action
        else {
            return false;
        };
        *called = true;
        let reapply = *reapply;
        if let Some(closing) = *stop {
            self.action = Some(Action::Disconnect { closing });
            self.show(DisplayState::Disconnecting, None);
            return false;
        }
        if phase == Phase::Back && *current != Phase::Back {
            *current = Phase::Back;
            let target = from.clone();
            let display = if reapply {
                DisplayState::Reapplying { back: true }
            } else {
                DisplayState::Switching { target, back: true }
            };
            self.show(display, None);
        }
        true
    }

    /// Ends the switch and shows the notice for the result. If "Disconnect" took over, nothing changes. The
    /// display is set to the known final state; the following query of `/state` confirms it.
    pub fn finish_switch(&mut self, result: &SwitchResult) {
        let Some(Action::Switch { from, to, reapply, .. }) = &self.action else {
            return;
        };
        let (from, to, reapply) = (from.clone(), to.clone(), *reapply);
        self.action = None;
        self.notice = if reapply {
            // Success is reported by the message only; errors additionally stay in the status card.
            match result {
                SwitchResult::Switched => None,
                other => reapply_notice(other).map(|text| Notice { text, command: None }),
            }
        } else {
            result.notice(&from, &to)
        };
        self.count_notice();
        let connected = |server: String| DisplayState::Connected { server: Some(server) };
        match result {
            SwitchResult::Switched => self.show(connected(to), None),
            SwitchResult::Returned(_)
            | SwitchResult::Refused
            | SwitchResult::Unreachable
            | SwitchResult::Rejected(_)
            | SwitchResult::Untested(_)
            | SwitchResult::NotStarted(_) => self.show(connected(from), None),
            SwitchResult::Stranded(_) | SwitchResult::Unclear(_) | SwitchResult::Stopped(_) => {}
        }
    }

    /// Ends the connection attempt of Celata. The new state comes from a query of `/state` afterwards.
    pub fn finish_connect(&mut self, result: &ConnectResult) {
        // If "Disconnect and close" replaced the attempt, its sequence continues to apply.
        if matches!(self.action, Some(Action::Connect { .. })) {
            self.action = None;
            self.notice = result.notice();
            self.count_notice();
        }
    }

    /// Starts disconnecting. Without `closing` only from "Connected"; with `closing` ("Disconnect and close")
    /// also during a connection attempt or a block. Returns [`DisconnectStart::No`] if it is not possible now.
    pub fn begin_disconnect(&mut self, closing: bool) -> DisconnectStart {
        // During a switch, its thread takes over the disconnect. "Disconnect and close" may still extend an
        // already requested "Disconnect" to closing.
        if let Some(Action::Switch { stop, .. }) = &mut self.action {
            return match *stop {
                Some(true) => DisconnectStart::No,
                Some(false) if !closing => DisconnectStart::No,
                _ => {
                    *stop = Some(closing);
                    self.notice = None;
                    self.show(DisplayState::Disconnecting, None);
                    DisconnectStart::Deferred
                }
            };
        }
        let allowed = if closing {
            // "Disconnect and close" may replace a running unblock; both use `/destroy`.
            !matches!(self.action, Some(Action::Disconnect { .. }))
                && matches!(
                    close_decision(self.display.as_ref()),
                    CloseDecision::Ask(CloseDialog {
                        offers_disconnect: true,
                        ..
                    })
                )
        } else {
            self.view().can_disconnect
        };
        if !allowed {
            return DisconnectStart::No;
        }
        self.action = Some(Action::Disconnect { closing });
        self.notice = None;
        self.show(DisplayState::Disconnecting, None);
        DisconnectStart::Now
    }

    /// Asks before the block is lifted. Returns `false` if there is nothing to lift.
    pub fn ask_unblock(&mut self) -> bool {
        self.unblock_asked = self.view().can_unblock;
        self.unblock_asked
    }

    /// Cancels the question; the block stays.
    pub fn cancel_unblock(&mut self) {
        self.unblock_asked = false;
    }

    /// Confirms lifting the block. Returns `true` if `/destroy` should follow now: only after the question,
    /// and only if the block still exists.
    pub fn confirm_unblock(&mut self) -> bool {
        let confirmed = self.unblock_asked && self.view().can_unblock;
        self.unblock_asked = false;
        if confirmed {
            // Also replaces a connection attempt started from the blocked state; `/destroy` waits for its end.
            self.action = Some(Action::Unblock);
            self.notice = None;
            self.show(DisplayState::Disconnecting, None);
        }
        confirmed
    }

    /// Ends lifting the block. If `/destroy` fails, Celata shows the emergency exit `systemctl stop`.
    pub fn finish_unblock(&mut self, result: &Result<(), Error>) {
        // If "Disconnect and close" took over, its result decides.
        if self.action != Some(Action::Unblock) {
            return;
        }
        self.action = None;
        self.notice = Some(match result {
            Ok(()) => Notice {
                text: Phrase::key(Key::NoticeUnblocked),
                command: None,
            },
            Err(error) => Notice {
                text: Phrase::with_reason(Phrase::key(Key::NoticeUnblockFailed), error.phrase()),
                command: Some(STOP_COMMAND),
            },
        });
        self.count_notice();
    }

    /// Ends disconnecting. Returns `true` if the window should close now. If "Disconnect and close" fails,
    /// the window stays open and shows the error and the emergency exit.
    pub fn finish_disconnect(&mut self, result: &Result<(), Error>) -> bool {
        let closing = matches!(self.action, Some(Action::Disconnect { closing: true }));
        self.action = None;
        match result {
            Ok(()) => closing,
            Err(error) => {
                let text = if closing {
                    Key::NoticeCloseDisconnectFailed
                } else {
                    Key::NoticeDisconnectFailed
                };
                self.notice = Some(Notice {
                    text: Phrase::with_reason(Phrase::key(text), error.phrase()),
                    command: Some(STOP_COMMAND),
                });
                self.count_notice();
                false
            }
        }
    }

    /// Processes a report. Returns the answer to the observation and whether the display changes.
    pub fn observe(&mut self, observation: Observation) -> (Flow, bool) {
        let before = self.view();
        if matches!(observation, Observation::State { .. } | Observation::Busy { .. }) {
            self.service_answered();
        }
        let flow = match observation {
            Observation::Version(version) => {
                self.version_warning = (version != EXPECTED_VERSION).then(|| {
                    Phrase::fill(
                        Key::VersionWarning,
                        [Phrase::raw(&version), Phrase::raw(EXPECTED_VERSION)],
                    )
                });
                Flow::Continue
            }
            Observation::State { state, blocked } => match evaluate(&state, blocked, self.own_connect_from_free()) {
                Update::Keep if state.code == "external ips" => {
                    let (v4, v6) = (state.external_v4.as_deref(), state.external_v6.as_deref());
                    self.external_ip.answer(v4, v6, Instant::now());
                    Flow::Continue
                }
                Update::Show(display) => {
                    let display = self.during_connect(display);
                    // Only on the transition to "Connected": the notice after a way back or a refused switch should
                    // survive the confirmation by `/state`.
                    let entering = !matches!(self.display, Some(DisplayState::Connected { .. }));
                    if entering && matches!(display, DisplayState::Connected { .. }) {
                        self.notice = None;
                    }
                    let connected = matches!(display, DisplayState::Connected { .. });
                    self.show(display, None);
                    if connected && state.code == "connected" {
                        self.service = Some(state);
                    }
                    Flow::Continue
                }
                Update::Keep => Flow::Continue,
                Update::Refresh => Flow::Refresh,
            },
            Observation::Busy { blocked } => {
                // HTTP 409: a connection is being set up or torn down. If the state is known, it stays.
                if !self.display.as_ref().is_some_and(DisplayState::service_reachable) {
                    let connecting = ServiceState {
                        code: "connecting".to_owned(),
                        ..ServiceState::default()
                    };
                    if let Update::Show(display) = evaluate(&connecting, blocked, self.own_connect_from_free()) {
                        self.show(display, None);
                    }
                }
                Flow::Continue
            }
            Observation::Failed { error, blocked } => {
                if !self.during_start(&error, blocked) {
                    let detail = match error {
                        Error::NotRunning | Error::PermissionDenied => None,
                        ref other => Some(Phrase::fill(Key::DetailReason, [other.phrase()])),
                    };
                    let display = match error {
                        // Without the group the socket could not be used even if the service ran.
                        Error::NotRunning | Error::PermissionDenied if self.start.group == GroupCheck::NotMember => {
                            DisplayState::NoPermission
                        }
                        _ => evaluate_error(&error, blocked),
                    };
                    self.show(display, detail);
                }
                Flow::Continue
            }
        };
        (flow, self.view() != before)
    }

    // --- Start of the service ---

    /// The first answer of the service ends every start: the reason disappears, and the target of a wish to
    /// connect is released.
    fn service_answered(&mut self) {
        if matches!(
            self.start.phase,
            StartPhase::Checking { .. } | StartPhase::Running { .. }
        ) {
            self.start.phase = StartPhase::Idle;
        }
        if let Some(target) = self.start.then.take() {
            self.start.connect_due = Some(target);
        }
        self.start.failure = None;
    }

    /// An error of the socket while a start runs. Returns `true` if it was handled here: at program start the
    /// display stays "Checking status …", during a start it is "Starting service …". Only once the unit is
    /// active does "No permission" count again.
    fn during_start(&mut self, error: &Error, blocked: bool) -> bool {
        let missing = matches!(error, Error::NotRunning | Error::PermissionDenied);
        let starting = DisplayState::ServiceStarting { blocked };
        match self.start.phase {
            StartPhase::Checking { starting: false } if missing => true,
            StartPhase::Checking { starting: true } | StartPhase::Running { unit_ready: false, .. } if missing => {
                self.show(starting, None);
                true
            }
            StartPhase::Running { unit_ready: true, .. } if *error == Error::NotRunning => {
                self.show(starting, None);
                true
            }
            StartPhase::Running { unit_ready: true, .. } if *error == Error::PermissionDenied => {
                // The service runs, and its socket refuses this process: the usual "No permission".
                self.end_start(None);
                self.start.reveal = true;
                false
            }
            _ => false,
        }
    }

    /// Ends a start without an answer of the service: back to "Service unavailable" (with the reason, if any).
    fn end_start(&mut self, failure: Option<StartFailure>) {
        self.start.phase = StartPhase::Idle;
        self.start.then = None;
        if let Some(failure) = failure {
            self.start.failure = Some(failure);
            self.start.reveal = true;
        }
        if let Some(DisplayState::ServiceStarting { blocked }) = self.display {
            let display = if blocked {
                DisplayState::ServiceUnreachableBlocked
            } else {
                DisplayState::ServiceUnreachable
            };
            self.show(display, None);
        }
    }

    /// "Starting service …" from an unreachable display, with the block as it is shown.
    fn show_starting(&mut self) {
        let blocked = match self.display {
            Some(DisplayState::ServiceUnreachableBlocked) => true,
            Some(DisplayState::ServiceStarting { blocked }) => blocked,
            Some(DisplayState::ServiceUnreachable) => false,
            _ => return,
        };
        self.show(DisplayState::ServiceStarting { blocked }, None);
    }

    /// The check at program start, on every way of starting Celata. Returns `false` if it ran already: there is
    /// only one per run.
    pub fn begin_startup_check(&mut self) -> bool {
        if self.start.checked || self.start.phase != StartPhase::Idle {
            return false;
        }
        self.start.checked = true;
        self.start.phase = StartPhase::Checking { starting: false };
        true
    }

    /// A wish to connect while the service is unreachable: starts the service first and remembers `then`, the
    /// target to connect to after its first answer. Only with `StatusView::start_on_connect`.
    pub fn request_start(&mut self, then: Option<String>) -> bool {
        if !self.view().start_on_connect {
            return false;
        }
        self.start.failure = None;
        self.start.then = then;
        self.start.phase = StartPhase::Checking { starting: true };
        self.notice = None;
        self.show_starting();
        true
    }

    /// The decision before the start, read from systemd and the group (`unit::before_start`). Returns `true` if
    /// `StartUnit` should follow now. Without a running check (for example because the service answered in the
    /// meantime) it changes nothing.
    pub fn startup_decided(&mut self, before: Before, group: GroupCheck) -> bool {
        if !matches!(self.start.phase, StartPhase::Checking { .. }) {
            return false;
        }
        self.start.group = group;
        let running = StartPhase::Running {
            since: Instant::now(),
            unit_ready: false,
            own: false,
        };
        match before {
            Before::Start => {
                self.start.phase = running;
                self.show_starting();
                true
            }
            Before::Wait => {
                self.start.phase = running;
                self.show_starting();
                false
            }
            // The socket decides as before.
            Before::Running => {
                self.start.phase = StartPhase::Idle;
                self.start.then = None;
                false
            }
            Before::NotInstalled => {
                self.end_start(Some(StartFailure::NotInstalled));
                false
            }
            Before::NoSystemBus => {
                self.end_start(Some(StartFailure::NoSystemBus));
                false
            }
            Before::NoPermission => {
                self.start.group = GroupCheck::NotMember;
                self.end_start(None);
                self.start.reveal = true;
                self.show(DisplayState::NoPermission, None);
                false
            }
        }
    }

    /// The answer to `StartUnit` without a password prompt.
    pub fn start_answered(&mut self, result: Result<(), StartFailure>) {
        let StartPhase::Running { own: false, .. } = self.start.phase else {
            return;
        };
        self.start.permit = self.start.permit.after(&result);
        match result {
            Ok(()) => {
                self.start.phase = StartPhase::Running {
                    since: Instant::now(),
                    unit_ready: false,
                    own: true,
                };
            }
            Err(failure) => self.end_start(Some(failure)),
        }
    }

    /// The state of the unit during a start, every second.
    pub fn start_progress(&mut self, progress: Progress) {
        let StartPhase::Running { since, own, .. } = self.start.phase else {
            return;
        };
        match progress {
            Progress::Waiting => {}
            Progress::Ready => {
                self.start.phase = StartPhase::Running {
                    since,
                    unit_ready: true,
                    own,
                };
            }
            // Only waited for a start or stop of someone else: no reason, the next wish to connect decides anew.
            Progress::Failed if !own => self.end_start(None),
            Progress::Failed => self.end_start(Some(StartFailure::Failed)),
            Progress::TimedOut => self.end_start(Some(StartFailure::NoAnswer)),
        }
    }

    /// Like [`Monitor::start_progress`], but only for the start that began at `since`: a reading taken for a start
    /// that has ended meanwhile changes nothing, even if a new start runs now. Returns whether that start is still
    /// running afterwards, so that the thread following it knows when to stop.
    pub fn start_progress_for(&mut self, since: Instant, progress: Progress) -> bool {
        if self.start_since() != Some(since) {
            return false;
        }
        self.start_progress(progress);
        self.start_since() == Some(since)
    }

    /// Since when the running start waits, `None` without one. The thread "service-start" polls while it is set.
    pub fn start_since(&self) -> Option<Instant> {
        match self.start.phase {
            StartPhase::Running { since, .. } => Some(since),
            _ => None,
        }
    }

    /// The button "Start service": only while the reason shown is "needs a password" and nothing runs. Returns
    /// whether the start with a password prompt should follow now.
    pub fn begin_interactive_start(&mut self) -> bool {
        if self.view().start_button != StartButton::Ready {
            return false;
        }
        self.start.phase = StartPhase::Asking;
        true
    }

    /// The result of the start with a password prompt. Cancelling changes nothing and sets no message.
    pub fn interactive_answered(&mut self, result: Interactive) {
        if self.start.phase != StartPhase::Asking {
            return;
        }
        self.start.phase = StartPhase::Idle;
        match result {
            Interactive::Started => {
                self.start.failure = None;
                self.start.phase = StartPhase::Running {
                    since: Instant::now(),
                    unit_ready: false,
                    own: true,
                };
                self.show_starting();
            }
            Interactive::Dismissed => {}
            Interactive::NoPrompt => {
                self.notice = Some(Notice {
                    text: Phrase::key(Key::StartNoPromptNotice),
                    command: Some(START_COMMAND),
                });
                self.count_notice();
            }
            Interactive::Failed(failure) => self.end_start(Some(failure)),
        }
    }

    /// The target of the wish to connect, once, after the first answer of the service that it started.
    pub fn take_connect_after_start(&mut self) -> Option<String> {
        self.start.connect_due.take()
    }

    /// Once per start that ended with a reason: bring a hidden window back.
    pub fn take_reveal(&mut self) -> bool {
        std::mem::take(&mut self.start.reveal)
    }

    /// Reason shown in the status card: the last failure, otherwise "needs a password" if that is known for this
    /// session and the service is unreachable.
    fn shown_reason(&self, display: &DisplayState) -> Option<StartFailure> {
        if !matches!(
            display,
            DisplayState::ServiceUnreachable | DisplayState::ServiceUnreachableBlocked
        ) {
            return None;
        }
        self.start
            .failure
            .or((self.start.permit == Permit::NeedsPassword).then_some(StartFailure::NeedsPassword))
    }

    /// Connecting starts the service first: unreachable without a fault, nothing running, and no reason known
    /// against a start without a password.
    fn start_on_connect(&self, display: &DisplayState) -> bool {
        matches!(
            display,
            DisplayState::ServiceUnreachable | DisplayState::ServiceUnreachableBlocked
        ) && self.detail.is_none()
            && self.action.is_none()
            && self.start.phase == StartPhase::Idle
            && self.start.permit != Permit::NeedsPassword
            && self.start.group != GroupCheck::NotMember
            && !matches!(
                self.start.failure,
                Some(StartFailure::NotInstalled | StartFailure::NoSystemBus | StartFailure::NeedsPassword)
            )
    }

    /// For tests elsewhere: a start of the service is not possible in this session (no system bus), so that the
    /// display is the one from before Celata started the service itself.
    #[cfg(test)]
    pub(crate) fn without_start(&mut self) {
        assert!(self.begin_startup_check());
        self.startup_decided(Before::NoSystemBus, GroupCheck::Member);
        self.take_reveal();
    }

    /// "Unblock internet" is possible in "Internet blocked" and "Connecting, internet blocked", also during a
    /// connection attempt of Celata, so that there is always a way out. If the service is not reachable, only
    /// the emergency exit in the terminal remains.
    fn can_unblock(&self, display: &DisplayState) -> bool {
        matches!(display, DisplayState::Blocked | DisplayState::ConnectingBlocked)
            && matches!(self.action, None | Some(Action::Connect { .. }))
    }

    /// As long as the connection attempt of Celata is running, intermediate states such as `routed` keep
    /// showing "Connecting". Only the result of `/connect` decides about success or failure.
    fn during_connect(&self, display: DisplayState) -> DisplayState {
        if let Some(Action::Switch {
            from,
            to,
            phase,
            stop,
            reapply,
            ..
        }) = &self.action
        {
            let target = if *phase == Phase::Back { from } else { to };
            return during_switch(display, target, *phase, stop.is_some(), *reapply);
        }
        match (&self.action, display) {
            (Some(Action::Connect { was_free: true, .. }), DisplayState::NotConnected | DisplayState::Blocked) => {
                DisplayState::Connecting
            }
            (Some(Action::Connect { was_free: false, .. }), DisplayState::NotConnected | DisplayState::Blocked) => {
                DisplayState::ConnectingBlocked
            }
            (_, display) => display,
        }
    }

    fn show(&mut self, display: DisplayState, detail: Option<Phrase>) {
        // Every change of state discards the public address that was determined.
        let changed = self.display.as_ref().map(std::mem::discriminant) != Some(std::mem::discriminant(&display));
        if changed {
            self.external_ip = ExternalIp::Idle;
        }
        if !matches!(display, DisplayState::Connected { .. }) {
            self.service = None;
        }
        self.display = Some(display);
        self.detail = detail;
    }

    /// Determining the public address: returns `true` if `/externalIps` should follow now. Only in
    /// "Connected" or "Not connected" with no operation running, once per state (after a failure
    /// again after 30 seconds). Never in "Internet blocked": for the query the service briefly opens
    /// exception routes that bypass the block.
    pub fn begin_external_ip(&mut self, now: Instant) -> bool {
        let allowed = matches!(
            self.display,
            Some(DisplayState::Connected { .. } | DisplayState::NotConnected)
        ) && self.action.is_none();
        if !allowed || !self.external_ip.due(now) {
            return false;
        }
        self.external_ip = ExternalIp::Asking { since: now };
        true
    }

    /// The call to `/externalIps` failed.
    pub fn external_ip_failed(&mut self, now: Instant) {
        if matches!(self.external_ip, ExternalIp::Asking { .. }) {
            self.external_ip = ExternalIp::Failed { at: now };
        }
    }

    /// Checks the 10 second limit. Returns `true` if the display changes.
    pub fn expire_external_ip(&mut self, now: Instant) -> bool {
        self.external_ip.expire(now)
    }

    /// The display for the user interface.
    pub fn view(&self) -> StatusView {
        let Some(display) = &self.display else {
            return StatusView {
                message: Message {
                    title: Key::StateCheckingTitle,
                    text: Key::StateCheckingText,
                    command: None,
                    warning: false,
                },
                server: None,
                detail: None,
                version_warning: self.version_warning.clone(),
                notice: self.notice.clone(),
                can_connect: false,
                connecting: false,
                can_disconnect: false,
                can_switch: false,
                connected_host: String::new(),
                switching: false,
                switch_target: None,
                connect_target: None,
                can_unblock: false,
                unblock_asked: false,
                service: None,
                external_ip: self.external_ip.clone(),
                route_ready: false,
                routing: false,
                start_on_connect: false,
                start_button: StartButton::Hidden,
                fix_rule: false,
                guide_link: false,
            };
        };
        let mut message = display.message();
        // If the service runs but answers incorrectly, the start command does not help.
        if *display == DisplayState::ServiceUnreachable && self.detail.is_some() {
            message.title = Key::StateServiceFaultyTitle;
            message.text = Key::StateServiceFaultyText;
            message.command = Some(RESTART_COMMAND);
        }
        let reason = self.shown_reason(display);
        let start_on_connect = self.start_on_connect(display);
        if *display == DisplayState::ServiceUnreachable && self.detail.is_none() {
            let set = |message: &mut Message, title, text, command| {
                message.title = title;
                message.text = text;
                message.command = command;
            };
            match reason {
                Some(StartFailure::NeedsPassword) => set(
                    &mut message,
                    Key::StartNeedsPasswordTitle,
                    Key::StartNeedsPasswordText,
                    None,
                ),
                Some(StartFailure::NotInstalled) => set(
                    &mut message,
                    Key::StartNotInstalledTitle,
                    Key::StartNotInstalledText,
                    None,
                ),
                Some(StartFailure::Failed) => set(
                    &mut message,
                    Key::StartFailedTitle,
                    Key::StartFailedText,
                    Some(STATUS_COMMAND),
                ),
                None if start_on_connect => {
                    message.text = Key::StateServiceUnreachableAutoText;
                    message.command = None;
                }
                // "No answer" and "no system bus" keep the message with the command to start by hand.
                Some(StartFailure::NoAnswer | StartFailure::NoSystemBus) | None => {}
            }
        }
        let start_button = match (self.start.phase, reason) {
            (StartPhase::Asking, _) => StartButton::Busy,
            (StartPhase::Idle, Some(StartFailure::NeedsPassword)) if self.action.is_none() => StartButton::Ready,
            _ => StartButton::Hidden,
        };
        if let Some(command) = self.notice.as_ref().and_then(|notice| notice.command) {
            message.command = Some(command);
        }
        // The own connection attempt names its target: "Connecting to {0} …" (feature 013, for every way of
        // connecting).
        let connect_target = match (&self.action, display) {
            (Some(Action::Connect { target, .. }), DisplayState::Connecting | DisplayState::ConnectingBlocked) => {
                Some(target.clone())
            }
            _ => None,
        };
        if connect_target.is_some() {
            message.title = match display {
                DisplayState::ConnectingBlocked => Key::StateConnectingBlockedToTitle,
                _ => Key::StateConnectingToTitle,
            };
        }
        StatusView {
            message,
            server: match display {
                DisplayState::Connected { server } => server.clone(),
                _ => None,
            },
            detail: self.detail.clone(),
            version_warning: self.version_warning.clone(),
            notice: self.notice.clone(),
            can_connect: self.action.is_none() && matches!(display, DisplayState::NotConnected | DisplayState::Blocked),
            connecting: matches!(self.action, Some(Action::Connect { .. })),
            can_disconnect: (self.action.is_none() && matches!(display, DisplayState::Connected { .. }))
                || matches!(self.action, Some(Action::Switch { stop: None, .. })),
            can_switch: self.action.is_none() && self.connected_server().is_some(),
            connected_host: self.connected_server().unwrap_or_default().to_owned(),
            switching: matches!(self.action, Some(Action::Switch { .. })),
            switch_target: match display {
                DisplayState::Switching { target, .. } => Some(target.clone()),
                _ => None,
            },
            connect_target,
            can_unblock: self.can_unblock(display),
            unblock_asked: self.unblock_asked && self.can_unblock(display),
            service: self.service.clone(),
            external_ip: self.external_ip.clone(),
            route_ready: self.can_route(true),
            routing: self.action == Some(Action::Route),
            start_on_connect,
            start_button,
            fix_rule: reason == Some(StartFailure::NeedsPassword),
            guide_link: matches!(reason, Some(StartFailure::NeedsPassword | StartFailure::NotInstalled)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{Deutsch, Language};

    fn service(code: &str, host: Option<&str>) -> ServiceState {
        ServiceState {
            code: code.to_owned(),
            host: host.map(str::to_owned),
            ..ServiceState::default()
        }
    }

    fn shown(code: &str, routes: bool, own: bool) -> DisplayState {
        match evaluate(&service(code, None), routes, own) {
            Update::Show(state) => state,
            other => panic!("{code}: {other:?}"),
        }
    }

    #[test]
    fn frei_ohne_routen() {
        assert_eq!(shown("clean", false, false), DisplayState::NotConnected);
        assert_eq!(shown("routed", false, false), DisplayState::NotConnected);
    }

    #[test]
    fn gesperrt_mit_routen() {
        assert_eq!(shown("routed", true, false), DisplayState::Blocked);
        assert_eq!(shown("dpd timeout", true, false), DisplayState::Blocked);
        assert_eq!(shown("clean", true, false), DisplayState::Blocked);
    }

    #[test]
    fn aufbau_aus_dem_freien_zustand_gilt_nicht_als_sperre() {
        assert_eq!(shown("connecting", true, true), DisplayState::Connecting);
        assert_eq!(shown("dns lookup", true, true), DisplayState::Connecting);
        assert_eq!(shown("connecting", false, false), DisplayState::Connecting);
    }

    #[test]
    fn wiederverbindung_zeigt_die_sperre() {
        assert_eq!(shown("connecting", true, false), DisplayState::ConnectingBlocked);
        assert_eq!(shown("dns lookup", true, false), DisplayState::ConnectingBlocked);
    }

    #[test]
    fn verbunden_mit_servername_auch_mit_routen() {
        assert_eq!(
            evaluate(&service("connected", Some("nl.hideservers.net")), true, false),
            Update::Show(DisplayState::Connected {
                server: Some("nl.hideservers.net".to_owned())
            })
        );
    }

    #[test]
    fn wird_getrennt() {
        assert_eq!(shown("disconnecting", true, false), DisplayState::Disconnecting);
    }

    #[test]
    fn hinweis_codes_aendern_nichts() {
        for code in [
            "configuration get",
            "configuration set",
            "logs dumped",
            "token update",
            "token update done",
            "external ips",
        ] {
            assert_eq!(evaluate(&service(code, None), false, false), Update::Keep, "{code}");
        }
    }

    #[test]
    fn unbekannte_codes_fuehren_zur_neuen_abfrage() {
        assert_eq!(evaluate(&service("irgendwas", None), false, false), Update::Refresh);
    }

    #[test]
    fn verbindungsfehler() {
        assert_eq!(
            evaluate_error(&Error::NotRunning, false),
            DisplayState::ServiceUnreachable
        );
        assert_eq!(
            evaluate_error(&Error::NotRunning, true),
            DisplayState::ServiceUnreachableBlocked
        );
        assert_eq!(
            evaluate_error(&Error::PermissionDenied, true),
            DisplayState::NoPermission
        );
    }

    fn all() -> Vec<DisplayState> {
        vec![
            DisplayState::NotConnected,
            DisplayState::Connecting,
            DisplayState::ConnectingBlocked,
            DisplayState::Connected { server: None },
            DisplayState::Disconnecting,
            DisplayState::Blocked,
            DisplayState::ServiceUnreachable,
            DisplayState::ServiceUnreachableBlocked,
            DisplayState::NoPermission,
            DisplayState::Switching {
                target: "de.hideservers.net".to_owned(),
                back: false,
            },
            DisplayState::Switching {
                target: "nl.hideservers.net".to_owned(),
                back: true,
            },
            DisplayState::Reapplying { back: false },
            DisplayState::Reapplying { back: true },
            DisplayState::ServiceStarting { blocked: false },
            DisplayState::ServiceStarting { blocked: true },
        ]
    }

    #[test]
    fn jeder_zustand_hat_englischen_titel_und_text() {
        for state in all() {
            let message = state.message();
            for key in [message.title, message.text] {
                let english = crate::i18n::text(Language::En, key);
                assert!(!english.trim().is_empty(), "{state:?}");
                assert_ne!(english, key.de(), "{state:?}: {key:?} ist nicht übersetzt");
            }
        }
        // The warning about the block points to the red area in English as well.
        let blocked = crate::i18n::text(Language::En, DisplayState::Blocked.message().text);
        assert!(
            blocked.contains("kill switch") && blocked.contains("red area"),
            "{blocked}"
        );
    }

    #[test]
    fn hinweis_wechselt_mit_der_sprache() {
        let mut monitor = Monitor::new();
        monitor.observe(observed("clean", None, false));
        assert!(monitor.begin_connect(NL).is_some());
        monitor.finish_connect(&ConnectResult::FailedReleased(Error::Timeout));
        let notice = monitor.view().notice.expect("Hinweis").text;
        assert!(notice.de().contains("kein VPN aktiv"));
        assert!(
            notice
                .de()
                .contains("Grund: Der Dienst hat nicht rechtzeitig geantwortet.")
        );
        let english = notice.render(Language::En);
        assert!(english.contains("no VPN is active"), "{english}");
        assert!(
            english.contains("Reason: The service did not respond in time."),
            "{english}"
        );
    }

    #[test]
    fn jeder_zustand_hat_titel_und_text() {
        for state in all() {
            let message = state.message();
            assert!(
                !message.title.de().is_empty() && !message.text.de().is_empty(),
                "{state:?}"
            );
        }
    }

    #[test]
    fn fehlerzustaende_nennen_einen_naechsten_schritt() {
        // "Starting service …" is no failure: it ends by itself.
        for state in all()
            .iter()
            .filter(|state| !state.service_reachable() && !matches!(state, DisplayState::ServiceStarting { .. }))
        {
            let message = state.message();
            assert!(message.command.is_some(), "{state:?}");
        }
    }

    #[test]
    fn dienst_aus_zeigt_den_startbefehl() {
        assert_eq!(DisplayState::ServiceUnreachable.message().command, Some(START_COMMAND));
        assert_eq!(START_COMMAND, "sudo systemctl start celata-hide.me.service");
    }

    /// Commands and notices name the service and the group only under the current name.
    #[test]
    fn befehle_nennen_nur_den_neuen_namen() {
        assert_eq!(RESTART_COMMAND, "sudo systemctl restart celata-hide.me.service");
        assert_eq!(STOP_COMMAND, "sudo systemctl stop celata-hide.me.service");
        assert_eq!(GROUP_COMMAND, "sudo usermod -aG celata \"$USER\"");
        assert_eq!(START_COMMAND, "sudo systemctl start celata-hide.me.service");
        for command in [START_COMMAND, RESTART_COMMAND, STOP_COMMAND, GROUP_COMMAND] {
            assert_eq!(command.matches("celata").count(), 1, "{command}");
        }
        let text = DisplayState::NoPermission.message().text;
        let (en, de) = (
            crate::i18n::text(crate::i18n::Language::En, text),
            crate::i18n::text(crate::i18n::Language::De, text),
        );
        assert!(en.contains("“celata” group"), "{en}");
        assert!(de.contains("Gruppe „celata“"), "{de}");
    }

    /// The unit name is defined once; every command uses it.
    #[test]
    fn befehle_nennen_den_einen_dienstnamen() {
        assert_eq!(SERVICE_UNIT, "celata-hide.me.service");
        for (command, verb) in [
            (START_COMMAND, "start"),
            (RESTART_COMMAND, "restart"),
            (STOP_COMMAND, "stop"),
        ] {
            assert_eq!(command, format!("sudo systemctl {verb} {SERVICE_UNIT}"));
        }
        // Reading the state needs no `sudo`.
        assert_eq!(STATUS_COMMAND, format!("systemctl status {SERVICE_UNIT}"));
    }

    /// A server name from the service is shown without control characters and bounded.
    #[test]
    fn servername_des_dienstes_wird_bereinigt() {
        let odd = format!("nl.hideservers.net\n\u{1b}[31m{}", "x".repeat(1000));
        let Update::Show(DisplayState::Connected { server: Some(server) }) =
            evaluate(&service("connected", Some(&odd)), false, false)
        else {
            panic!("expected connected")
        };
        assert!(server.starts_with("nl.hideservers.net[31mxxx"), "{server}");
        assert_eq!(server.chars().count(), 253);
        // A usual name is unchanged, an empty one counts as unknown.
        assert_eq!(
            shown_server("de.hideservers.net"),
            Some("de.hideservers.net".to_owned())
        );
        assert_eq!(shown_server(" \n "), None);
    }

    fn shown_server(host: &str) -> Option<String> {
        match evaluate(&service("connected", Some(host)), false, false) {
            Update::Show(DisplayState::Connected { server }) => server,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn dienst_aus_mit_sperre_zeigt_den_notausgang() {
        let message = DisplayState::ServiceUnreachableBlocked.message();
        assert_eq!(message.command, Some(STOP_COMMAND));
        assert!(message.warning);
    }

    #[test]
    fn sperren_werden_hervorgehoben() {
        for state in all() {
            let blocked = matches!(
                state,
                DisplayState::Blocked
                    | DisplayState::ConnectingBlocked
                    | DisplayState::ServiceUnreachableBlocked
                    | DisplayState::ServiceStarting { blocked: true }
            );
            assert_eq!(state.message().warning, blocked, "{state:?}");
        }
    }

    fn observed(code: &str, host: Option<&str>, blocked: bool) -> Observation {
        Observation::State {
            state: service(code, host),
            blocked,
        }
    }

    #[test]
    fn monitor_zeigt_zuerst_die_pruefung() {
        let monitor = Monitor::new();
        assert_eq!(monitor.display(), None);
        assert_eq!(monitor.view().message.title.de(), "Zustand wird geprüft …");
    }

    #[test]
    fn monitor_zeigt_servernamen() {
        let mut monitor = Monitor::new();
        let (flow, changed) = monitor.observe(observed("connected", Some("nl.hideservers.net"), false));
        assert_eq!((flow, changed), (Flow::Continue, true));
        assert_eq!(monitor.view().server.as_deref(), Some("nl.hideservers.net"));
        assert_eq!(monitor.view().message.title.de(), "Verbunden");
    }

    #[test]
    fn monitor_hinweis_codes_aendern_nichts() {
        let mut monitor = Monitor::new();
        monitor.observe(observed("clean", None, false));
        assert_eq!(
            monitor.observe(observed("configuration set", None, false)),
            (Flow::Continue, false)
        );
        assert_eq!(monitor.display(), Some(&DisplayState::NotConnected));
    }

    #[test]
    fn monitor_fragt_bei_unbekanntem_code_neu_ab() {
        let mut monitor = Monitor::new();
        monitor.observe(observed("clean", None, false));
        assert_eq!(monitor.observe(observed("neu", None, false)), (Flow::Refresh, false));
    }

    #[test]
    fn monitor_versionsabweichung() {
        let mut monitor = Monitor::new();
        monitor.observe(Observation::Version(EXPECTED_VERSION.to_owned()));
        assert_eq!(monitor.view().version_warning, None);
        monitor.observe(Observation::Version("0.10.0".to_owned()));
        let warning = monitor.view().version_warning.unwrap().de();
        assert!(warning.contains("0.10.0") && warning.contains(EXPECTED_VERSION));
    }

    #[test]
    fn monitor_dienst_nicht_erreichbar() {
        let mut monitor = Monitor::new();
        monitor.observe(Observation::Failed {
            error: Error::NotRunning,
            blocked: false,
        });
        // Celata may start the service: connecting starts it, no command.
        let view = monitor.view();
        assert!(view.start_on_connect);
        assert_eq!(view.message.command, None);
        assert_eq!(view.message.text, Key::StateServiceUnreachableAutoText);
        assert_eq!(view.detail, None);
        // Without a possible start the command as before.
        let mut monitor = Monitor::new();
        monitor.without_start();
        monitor.observe(Observation::Failed {
            error: Error::NotRunning,
            blocked: false,
        });
        let view = monitor.view();
        assert!(!view.start_on_connect);
        assert_eq!(view.message.command, Some(START_COMMAND));
        assert_eq!(view.detail, None);

        monitor.observe(Observation::Failed {
            error: Error::Timeout,
            blocked: true,
        });
        let view = monitor.view();
        assert_eq!(monitor.display(), Some(&DisplayState::ServiceUnreachableBlocked));
        assert!(view.detail.unwrap().de().contains("nicht rechtzeitig"));
    }

    #[test]
    fn monitor_keine_berechtigung() {
        let mut monitor = Monitor::new();
        monitor.observe(Observation::Failed {
            error: Error::PermissionDenied,
            blocked: true,
        });
        assert_eq!(monitor.display(), Some(&DisplayState::NoPermission));
        assert_eq!(monitor.view().message.command, Some(GROUP_COMMAND));
    }

    #[test]
    fn monitor_beschaeftigt_behaelt_bekannten_zustand() {
        let mut monitor = Monitor::new();
        monitor.observe(observed("connected", Some("nl.hideservers.net"), false));
        assert_eq!(
            monitor.observe(Observation::Busy { blocked: false }),
            (Flow::Continue, false)
        );
    }

    #[test]
    fn monitor_beschaeftigt_beim_start_heisst_wird_verbunden() {
        let mut monitor = Monitor::new();
        monitor.observe(Observation::Busy { blocked: false });
        assert_eq!(monitor.display(), Some(&DisplayState::Connecting));

        let mut monitor = Monitor::new();
        monitor.observe(Observation::Failed {
            error: Error::NotRunning,
            blocked: false,
        });
        monitor.observe(Observation::Busy { blocked: true });
        assert_eq!(monitor.display(), Some(&DisplayState::ConnectingBlocked));
    }

    #[test]
    fn monitor_dienst_wieder_da() {
        let mut monitor = Monitor::new();
        monitor.observe(Observation::Failed {
            error: Error::Timeout,
            blocked: false,
        });
        monitor.observe(observed("clean", None, false));
        assert_eq!(monitor.display(), Some(&DisplayState::NotConnected));
        assert_eq!(monitor.view().detail, None);
    }

    // --- Start of the service (feature 015) ---

    fn failed(error: Error, blocked: bool) -> Observation {
        Observation::Failed { error, blocked }
    }

    fn not_running(blocked: bool) -> Observation {
        failed(Error::NotRunning, blocked)
    }

    /// Unreachable service, nothing started yet.
    fn unreachable(blocked: bool) -> Monitor {
        let mut monitor = Monitor::new();
        monitor.observe(not_running(blocked));
        monitor
    }

    /// A start at program start that systemd accepted.
    fn started_at_program_start() -> Monitor {
        let mut monitor = Monitor::new();
        assert!(monitor.begin_startup_check());
        assert!(monitor.startup_decided(Before::Start, GroupCheck::Member));
        monitor.start_answered(Ok(()));
        monitor
    }

    #[test]
    fn dienst_wird_gestartet_ist_ein_eigener_zustand() {
        for blocked in [false, true] {
            let state = DisplayState::ServiceStarting { blocked };
            assert!(!state.service_reachable());
            let message = state.message();
            assert_eq!(message.command, None);
            assert_eq!(message.warning, blocked);
            let (title, text) = if blocked {
                (
                    Key::StateServiceStartingBlockedTitle,
                    Key::StateServiceStartingBlockedText,
                )
            } else {
                (Key::StateServiceStartingTitle, Key::StateServiceStartingText)
            };
            assert_eq!((message.title, message.text), (title, text));
        }
        assert_eq!(
            DisplayState::ServiceStarting { blocked: false }.message().title.de(),
            "Dienst wird gestartet …"
        );
        // Closing as for "Service unavailable", with the block as for "…, internet blocked".
        assert_eq!(
            close_decision(Some(&DisplayState::ServiceStarting { blocked: false })),
            CloseDecision::Close
        );
        assert_eq!(
            close_decision(Some(&DisplayState::ServiceStarting { blocked: true })),
            close_decision(Some(&DisplayState::ServiceUnreachableBlocked))
        );
        // Nothing to connect to while it starts.
        let mut monitor = unreachable(false);
        assert!(monitor.request_start(None));
        assert_eq!(
            monitor.display(),
            Some(&DisplayState::ServiceStarting { blocked: false })
        );
        let view = monitor.view();
        assert!(!view.can_connect && !view.start_on_connect);
        assert_eq!(view.start_button, StartButton::Hidden);
    }

    #[test]
    fn beim_programmstart_zeigen_fehler_des_sockets_die_pruefung() {
        let mut monitor = Monitor::new();
        assert!(monitor.begin_startup_check());
        for error in [Error::NotRunning, Error::PermissionDenied] {
            monitor.observe(failed(error, false));
            assert_eq!(monitor.display(), None);
            assert_eq!(monitor.view().message.title, Key::StateCheckingTitle);
        }
        // After the decision to start: "Starting service …", with the block from the routes.
        assert!(monitor.startup_decided(Before::Start, GroupCheck::Member));
        monitor.observe(not_running(false));
        assert_eq!(
            monitor.display(),
            Some(&DisplayState::ServiceStarting { blocked: false })
        );
        monitor.observe(failed(Error::PermissionDenied, true));
        assert_eq!(
            monitor.display(),
            Some(&DisplayState::ServiceStarting { blocked: true })
        );
        // Waiting for a start of someone else looks the same, without `StartUnit`.
        let mut waiting = Monitor::new();
        assert!(waiting.begin_startup_check());
        assert!(!waiting.startup_decided(Before::Wait, GroupCheck::Member));
        waiting.observe(failed(Error::PermissionDenied, false));
        assert_eq!(
            waiting.display(),
            Some(&DisplayState::ServiceStarting { blocked: false })
        );
    }

    #[test]
    fn erst_die_aktive_unit_macht_keine_berechtigung_wieder_moeglich() {
        let mut monitor = started_at_program_start();
        assert_eq!(monitor.start.permit, Permit::Allowed);
        // The socket belongs to root for a moment: still "Starting service …".
        monitor.observe(failed(Error::PermissionDenied, false));
        assert_eq!(
            monitor.display(),
            Some(&DisplayState::ServiceStarting { blocked: false })
        );
        monitor.start_progress(Progress::Ready);
        monitor.observe(not_running(false));
        assert_eq!(
            monitor.display(),
            Some(&DisplayState::ServiceStarting { blocked: false })
        );
        monitor.observe(failed(Error::PermissionDenied, false));
        assert_eq!(monitor.display(), Some(&DisplayState::NoPermission));
        assert!(monitor.take_reveal());
        assert_eq!(monitor.start_since(), None);
    }

    #[test]
    fn erste_antwort_beendet_den_start_ohne_aktion() {
        let mut monitor = started_at_program_start();
        monitor.observe(not_running(false));
        assert!(monitor.start_since().is_some());
        monitor.observe(observed("clean", None, false));
        assert_eq!(monitor.display(), Some(&DisplayState::NotConnected));
        assert_eq!(monitor.start_since(), None);
        assert_eq!(monitor.start.failure, None);
        // No connecting by itself, and a hidden window stays hidden.
        assert_eq!(monitor.take_connect_after_start(), None);
        assert!(!monitor.take_reveal());
        assert!(monitor.action.is_none());
        // Only one check per run.
        assert!(!monitor.begin_startup_check());
    }

    #[test]
    fn laufender_dienst_wird_nicht_gestartet() {
        let mut monitor = Monitor::new();
        assert!(monitor.begin_startup_check());
        assert!(!monitor.begin_startup_check(), "a second check does nothing");
        assert!(!monitor.startup_decided(Before::Running, GroupCheck::Member));
        assert_eq!(monitor.start_since(), None);
        // The socket decides as before.
        monitor.observe(observed("clean", None, false));
        assert_eq!(monitor.display(), Some(&DisplayState::NotConnected));
        // A decision after an answer of the service changes nothing.
        let mut answered = Monitor::new();
        assert!(answered.begin_startup_check());
        answered.observe(observed("clean", None, false));
        assert!(!answered.startup_decided(Before::Start, GroupCheck::Member));
        assert_eq!(answered.start_since(), None);
    }

    #[test]
    fn verschwundener_dienst_wird_nur_auf_wunsch_gestartet() {
        let mut monitor = started_at_program_start();
        monitor.observe(observed("clean", None, false));
        for blocked in [false, true] {
            for _ in 0..120 {
                monitor.observe(not_running(blocked));
            }
            let expected = if blocked {
                DisplayState::ServiceUnreachableBlocked
            } else {
                DisplayState::ServiceUnreachable
            };
            assert_eq!(monitor.display(), Some(&expected));
            assert_eq!(monitor.start_since(), None, "no start by itself");
            let view = monitor.view();
            assert!(view.start_on_connect);
            assert!(!view.can_connect);
        }
        let view = unreachable(false).view();
        assert_eq!(view.message.title, Key::StateServiceUnreachableTitle);
        assert_eq!(view.message.text, Key::StateServiceUnreachableAutoText);
        assert_eq!(view.message.command, None);
    }

    #[test]
    fn starten_beim_verbinden_je_erlaubnis_und_grund() {
        // Unknown and allowed permit: connecting starts the service.
        assert!(unreachable(false).view().start_on_connect);
        assert!(unreachable(true).view().start_on_connect);
        let mut allowed = started_at_program_start();
        allowed.observe(observed("clean", None, false));
        allowed.observe(not_running(false));
        assert!(allowed.view().start_on_connect);
        // Reachable service, no permission, a running start: no.
        assert!(!monitor_in("clean", false).view().start_on_connect);
        let mut no_permission = Monitor::new();
        no_permission.observe(failed(Error::PermissionDenied, false));
        assert!(!no_permission.view().start_on_connect);
        let mut starting = unreachable(false);
        assert!(starting.request_start(None));
        assert!(!starting.view().start_on_connect);
        // A faulty service is not started.
        let mut faulty = Monitor::new();
        faulty.observe(failed(Error::Timeout, false));
        assert!(!faulty.view().start_on_connect);
        // Reasons.
        for (failure, expected) in [
            (StartFailure::NeedsPassword, false),
            (StartFailure::NotInstalled, false),
            (StartFailure::NoSystemBus, false),
            (StartFailure::Failed, true),
            (StartFailure::NoAnswer, true),
        ] {
            let mut monitor = unreachable(false);
            assert!(monitor.request_start(None));
            assert!(monitor.startup_decided(Before::Start, GroupCheck::Member));
            match failure {
                StartFailure::Failed | StartFailure::NoAnswer => {
                    monitor.start_answered(Ok(()));
                    monitor.start_progress(if failure == StartFailure::Failed {
                        Progress::Failed
                    } else {
                        Progress::TimedOut
                    });
                }
                other => monitor.start_answered(Err(other)),
            }
            monitor.observe(not_running(false));
            assert_eq!(monitor.view().start_on_connect, expected, "{failure:?}");
        }
    }

    #[test]
    fn ziel_des_verbindungswunsches_genau_einmal() {
        let mut monitor = unreachable(false);
        assert!(!monitor_in("clean", false).request_start(Some(NL.into())));
        assert!(monitor.request_start(Some(NL.into())));
        assert!(
            !monitor.request_start(Some("de.hideservers.net".into())),
            "only one start"
        );
        assert!(monitor.startup_decided(Before::Start, GroupCheck::Member));
        monitor.start_answered(Ok(()));
        monitor.observe(not_running(false));
        assert_eq!(monitor.take_connect_after_start(), None, "not before the answer");
        monitor.observe(observed("clean", None, false));
        assert_eq!(monitor.take_connect_after_start(), Some(NL.to_owned()));
        assert_eq!(monitor.take_connect_after_start(), None);
        assert!(monitor.view().can_connect);
        // A failed start discards the target.
        let mut failed_start = unreachable(false);
        assert!(failed_start.request_start(Some(NL.into())));
        assert!(failed_start.startup_decided(Before::Start, GroupCheck::Member));
        failed_start.start_answered(Err(StartFailure::NeedsPassword));
        failed_start.observe(observed("clean", None, false));
        assert_eq!(failed_start.take_connect_after_start(), None);
        // As does a unit that is not set up.
        let mut missing = unreachable(false);
        assert!(missing.request_start(Some(NL.into())));
        assert!(!missing.startup_decided(Before::NotInstalled, GroupCheck::Member));
        missing.observe(observed("clean", None, false));
        assert_eq!(missing.take_connect_after_start(), None);
    }

    /// Unreachable display after a start that ended with `failure`.
    fn after_failure(failure: StartFailure, blocked: bool) -> Monitor {
        let mut monitor = unreachable(blocked);
        assert!(monitor.request_start(None));
        match failure {
            StartFailure::NotInstalled => {
                monitor.startup_decided(Before::NotInstalled, GroupCheck::Member);
            }
            StartFailure::NoSystemBus => {
                monitor.startup_decided(Before::NoSystemBus, GroupCheck::Member);
            }
            StartFailure::NeedsPassword => {
                monitor.startup_decided(Before::Start, GroupCheck::Member);
                monitor.start_answered(Err(StartFailure::NeedsPassword));
            }
            StartFailure::Failed => {
                monitor.startup_decided(Before::Start, GroupCheck::Member);
                monitor.start_answered(Ok(()));
                monitor.start_progress(Progress::Failed);
            }
            StartFailure::NoAnswer => {
                monitor.startup_decided(Before::Start, GroupCheck::Member);
                monitor.start_answered(Ok(()));
                monitor.start_progress(Progress::TimedOut);
            }
        }
        monitor.observe(not_running(blocked));
        monitor
    }

    #[test]
    fn jeder_grund_nennt_den_naechsten_schritt() {
        let cases = [
            (
                StartFailure::NeedsPassword,
                Key::StartNeedsPasswordTitle,
                Key::StartNeedsPasswordText,
                None,
                (StartButton::Ready, true, true),
            ),
            (
                StartFailure::NotInstalled,
                Key::StartNotInstalledTitle,
                Key::StartNotInstalledText,
                None,
                (StartButton::Hidden, false, true),
            ),
            (
                StartFailure::Failed,
                Key::StartFailedTitle,
                Key::StartFailedText,
                Some(STATUS_COMMAND),
                (StartButton::Hidden, false, false),
            ),
            (
                StartFailure::NoAnswer,
                Key::StateServiceUnreachableTitle,
                Key::StateServiceUnreachableText,
                Some(START_COMMAND),
                (StartButton::Hidden, false, false),
            ),
            (
                StartFailure::NoSystemBus,
                Key::StateServiceUnreachableTitle,
                Key::StateServiceUnreachableText,
                Some(START_COMMAND),
                (StartButton::Hidden, false, false),
            ),
        ];
        for (failure, title, text, command, (button, fix_rule, guide_link)) in cases {
            let mut monitor = after_failure(failure, false);
            assert_eq!(monitor.display(), Some(&DisplayState::ServiceUnreachable));
            assert!(monitor.take_reveal(), "{failure:?}: window comes back");
            assert!(!monitor.take_reveal(), "{failure:?}: only once");
            let view = monitor.view();
            assert_eq!((view.message.title, view.message.text), (title, text), "{failure:?}");
            assert_eq!(view.message.command, command, "{failure:?}");
            assert_eq!(
                (view.start_button, view.fix_rule, view.guide_link),
                (button, fix_rule, guide_link),
                "{failure:?}"
            );
            // Never a start by itself after a reason.
            for _ in 0..30 {
                monitor.observe(not_running(false));
            }
            assert_eq!(monitor.start_since(), None, "{failure:?}");
            // With the block the message stays; button, section and link come in addition.
            let blocked = after_failure(failure, true).view();
            assert_eq!(
                blocked.message.title,
                Key::StateServiceUnreachableBlockedTitle,
                "{failure:?}"
            );
            assert_eq!(blocked.message.command, Some(STOP_COMMAND), "{failure:?}");
            assert_eq!(
                (blocked.start_button, blocked.fix_rule, blocked.guide_link),
                (button, fix_rule, guide_link),
                "{failure:?}"
            );
        }
        // A faulty service keeps its message.
        let mut faulty = Monitor::new();
        faulty.observe(failed(Error::Timeout, false));
        assert_eq!(faulty.view().message.title, Key::StateServiceFaultyTitle);
        // The first answer removes the reason.
        let mut monitor = after_failure(StartFailure::Failed, false);
        monitor.observe(observed("clean", None, false));
        monitor.observe(not_running(false));
        assert_eq!(monitor.view().message.text, Key::StateServiceUnreachableAutoText);
    }

    #[test]
    fn ohne_gruppe_keine_berechtigung_ohne_startversuch() {
        let mut monitor = Monitor::new();
        assert!(monitor.begin_startup_check());
        assert!(!monitor.startup_decided(Before::NoPermission, GroupCheck::NotMember));
        assert_eq!(monitor.display(), Some(&DisplayState::NoPermission));
        assert!(monitor.take_reveal());
        monitor.observe(not_running(false));
        assert_eq!(monitor.display(), Some(&DisplayState::NoPermission));
        let view = monitor.view();
        assert_eq!(view.message.command, Some(GROUP_COMMAND));
        assert!(!view.start_on_connect);
        assert_eq!(monitor.start_since(), None);
    }

    #[test]
    fn knopf_dienst_starten() {
        // Only with the reason "needs a password".
        for failure in [StartFailure::NotInstalled, StartFailure::Failed, StartFailure::NoAnswer] {
            assert!(!after_failure(failure, false).begin_interactive_start(), "{failure:?}");
        }
        assert!(!unreachable(false).begin_interactive_start());
        let mut monitor = after_failure(StartFailure::NeedsPassword, false);
        assert!(monitor.begin_interactive_start());
        assert_eq!(monitor.view().start_button, StartButton::Busy);
        assert!(monitor.view().fix_rule);
        assert!(!monitor.begin_interactive_start(), "one prompt at a time");
        // Cancelled or wrong password: nothing changes, no message.
        let count = monitor.notice_count();
        monitor.interactive_answered(Interactive::Dismissed);
        assert_eq!(monitor.view().start_button, StartButton::Ready);
        assert_eq!(monitor.view().notice, None);
        assert_eq!(monitor.notice_count(), count);
        // No agent for the prompt: one message with the command.
        assert!(monitor.begin_interactive_start());
        monitor.interactive_answered(Interactive::NoPrompt);
        let view = monitor.view();
        assert_eq!(view.start_button, StartButton::Ready);
        assert_eq!(
            view.notice.as_ref().map(|notice| notice.command),
            Some(Some(START_COMMAND))
        );
        assert_eq!(
            view.notice.map(|notice| notice.text),
            Some(Phrase::key(Key::StartNoPromptNotice))
        );
        assert_eq!(monitor.notice_count(), count + 1);
        // Another reason from the prompt.
        assert!(monitor.begin_interactive_start());
        monitor.interactive_answered(Interactive::Failed(StartFailure::NotInstalled));
        assert_eq!(monitor.view().message.title, Key::StartNotInstalledTitle);
        // Started with the password: "Starting service …", then "Not connected".
        let mut monitor = after_failure(StartFailure::NeedsPassword, false);
        assert!(monitor.begin_interactive_start());
        monitor.interactive_answered(Interactive::Started);
        assert_eq!(
            monitor.display(),
            Some(&DisplayState::ServiceStarting { blocked: false })
        );
        assert!(monitor.start_since().is_some());
        monitor.observe(observed("clean", None, false));
        assert_eq!(monitor.display(), Some(&DisplayState::NotConnected));
        // A start with the password does not change the permit: if the service goes again, the button comes back.
        assert_eq!(monitor.start.permit, Permit::NeedsPassword);
        monitor.observe(not_running(false));
        let view = monitor.view();
        assert_eq!(view.message.title, Key::StartNeedsPasswordTitle);
        assert_eq!(view.start_button, StartButton::Ready);
        assert!(view.fix_rule && view.guide_link);
        assert!(!view.start_on_connect);
        assert!(monitor.begin_interactive_start());
    }

    #[test]
    fn fortschritt_gilt_nur_fuer_den_verfolgten_start() {
        // A start ends with the first answer of the service, a new one begins at once (the service vanished again).
        let mut monitor = started_at_program_start();
        let old = monitor.start_since().expect("start runs");
        monitor.observe(observed("clean", None, false));
        monitor.observe(not_running(false));
        std::thread::sleep(std::time::Duration::from_millis(2));
        assert!(monitor.request_start(None));
        assert!(monitor.startup_decided(Before::Start, GroupCheck::Member));
        monitor.start_answered(Ok(()));
        let new = monitor.start_since().expect("new start runs");
        assert_ne!(old, new);
        // The thread of the old start reports late, with its own times: no effect on the new start.
        for progress in [Progress::TimedOut, Progress::Failed, Progress::Ready] {
            assert!(!monitor.start_progress_for(old, progress), "{progress:?}");
        }
        assert_eq!(monitor.start_since(), Some(new));
        assert_eq!(monitor.start.failure, None);
        assert!(!monitor.take_reveal());
        // The thread of the new start goes on while it runs and learns when it has ended.
        assert!(monitor.start_progress_for(new, Progress::Waiting));
        assert!(monitor.start_progress_for(new, Progress::Ready));
        assert!(!monitor.start_progress_for(new, Progress::TimedOut));
        assert_eq!(monitor.start.failure, Some(StartFailure::NoAnswer));
        assert!(!monitor.start_progress_for(new, Progress::Waiting));
    }

    #[test]
    fn nur_eigener_start_meldet_ein_scheitern() {
        // A start of someone else that ended: no reason, the next wish decides anew.
        let mut monitor = Monitor::new();
        assert!(monitor.begin_startup_check());
        assert!(!monitor.startup_decided(Before::Wait, GroupCheck::Member));
        monitor.start_progress(Progress::Failed);
        assert_eq!(monitor.start.failure, None);
        assert!(!monitor.take_reveal());
        monitor.observe(not_running(false));
        assert!(monitor.view().start_on_connect);
    }

    // --- Connecting ---

    fn monitor_in(code: &str, blocked: bool) -> Monitor {
        let mut monitor = Monitor::new();
        monitor.observe(observed(code, None, blocked));
        monitor
    }

    #[test]
    fn verbinden_nur_aus_frei_oder_gesperrt() {
        assert!(monitor_in("clean", false).view().can_connect);
        assert!(monitor_in("routed", true).view().can_connect);
        assert!(!monitor_in("connected", false).view().can_connect);
        assert!(!monitor_in("connecting", false).view().can_connect);
        assert!(!monitor_in("disconnecting", false).view().can_connect);
        assert!(!Monitor::new().view().can_connect);
        let mut unreachable = Monitor::new();
        unreachable.observe(Observation::Failed {
            error: Error::NotRunning,
            blocked: false,
        });
        assert!(!unreachable.view().can_connect);
    }

    #[test]
    fn ausgangszustand_wird_gemerkt() {
        let mut free = monitor_in("clean", false);
        assert_eq!(
            free.begin_connect("nl.hideservers.net"),
            Some(Start::Connect { was_free: true })
        );
        assert_eq!(free.display(), Some(&DisplayState::Connecting));

        let mut blocked = monitor_in("routed", true);
        assert_eq!(
            blocked.begin_connect("nl.hideservers.net"),
            Some(Start::Connect { was_free: false })
        );
        assert_eq!(blocked.display(), Some(&DisplayState::ConnectingBlocked));
    }

    #[test]
    fn doppeltes_ausloesen_startet_keinen_zweiten_aufbau() {
        let mut monitor = monitor_in("clean", false);
        assert_eq!(
            monitor.begin_connect("nl.hideservers.net"),
            Some(Start::Connect { was_free: true })
        );
        assert_eq!(monitor.begin_connect("nl.hideservers.net"), None);
        // An intermediate state of the service does not release "Connect" either.
        monitor.observe(observed("routed", None, true));
        assert_eq!(monitor.display(), Some(&DisplayState::Connecting));
        assert_eq!(monitor.begin_connect("nl.hideservers.net"), None);
    }

    #[test]
    fn eigener_aufbau_mit_sperr_routen_zeigt_keine_sperre() {
        let mut monitor = monitor_in("clean", false);
        monitor.begin_connect("nl.hideservers.net");
        monitor.observe(observed("connecting", None, true));
        assert_eq!(monitor.display(), Some(&DisplayState::Connecting));
        monitor.observe(observed("connected", Some("nl.hideservers.net"), true));
        assert!(matches!(monitor.display(), Some(DisplayState::Connected { .. })));
    }

    #[test]
    fn nach_dem_aufbau_gilt_wieder_der_dienstzustand() {
        let mut monitor = monitor_in("clean", false);
        monitor.begin_connect("nl.hideservers.net");
        monitor.finish_connect(&ConnectResult::FailedReleased(Error::Timeout));
        // Until `/state` has been queried again, the display stays as it is.
        assert_eq!(monitor.display(), Some(&DisplayState::Connecting));
        monitor.observe(observed("clean", None, false));
        let view = monitor.view();
        assert_eq!(monitor.display(), Some(&DisplayState::NotConnected));
        assert!(view.can_connect && !view.connecting);
        assert!(view.notice.unwrap().text.de().contains("kein VPN aktiv"));
    }

    #[test]
    fn hinweise_nach_fehlschlag() {
        let notice = |result: ConnectResult| result.notice().unwrap();
        assert!(
            notice(ConnectResult::FailedReleased(Error::Timeout))
                .text
                .de()
                .contains("kein VPN aktiv")
        );
        let blocked = notice(ConnectResult::FailedBlocked(Error::Timeout));
        assert!(blocked.text.de().contains("Sperre bleibt"));
        assert_eq!(blocked.command, None);
        assert_eq!(
            notice(ConnectResult::ReleaseFailed(Error::Timeout)).command,
            Some(STOP_COMMAND)
        );
        assert!(
            notice(ConnectResult::NotStarted(Error::Busy))
                .text
                .de()
                .contains("noch einmal")
        );
        assert_eq!(ConnectResult::Connected.notice(), None);
    }

    #[test]
    fn notausgang_aus_dem_hinweis_wird_angezeigt() {
        let mut monitor = monitor_in("clean", false);
        monitor.begin_connect("nl.hideservers.net");
        monitor.finish_connect(&ConnectResult::ReleaseFailed(Error::Timeout));
        monitor.observe(observed("routed", None, true));
        assert_eq!(monitor.view().message.command, Some(STOP_COMMAND));
    }

    #[test]
    fn erfolg_loescht_alten_hinweis() {
        let mut monitor = monitor_in("clean", false);
        monitor.begin_connect("nl.hideservers.net");
        monitor.finish_connect(&ConnectResult::FailedReleased(Error::Timeout));
        monitor.observe(observed("clean", None, false));
        monitor.begin_connect("nl.hideservers.net");
        assert_eq!(monitor.view().notice, None);
        monitor.finish_connect(&ConnectResult::Connected);
        monitor.observe(observed("connected", Some("nl.hideservers.net"), false));
        assert_eq!(monitor.view().notice, None);
    }

    // --- Disconnecting and closing ---

    #[test]
    fn trennen_nur_bei_verbindung() {
        assert!(monitor_in("connected", false).view().can_disconnect);
        for (code, blocked) in [
            ("clean", false),
            ("routed", true),
            ("connecting", false),
            ("disconnecting", false),
        ] {
            assert!(!monitor_in(code, blocked).view().can_disconnect, "{code}");
        }
    }

    #[test]
    fn trennen_zeigt_wird_getrennt_und_danach_nicht_verbunden() {
        let mut monitor = monitor_in("connected", false);
        assert_eq!(monitor.begin_disconnect(false), DisconnectStart::Now);
        assert_eq!(monitor.display(), Some(&DisplayState::Disconnecting));
        assert_eq!(
            monitor.begin_disconnect(false),
            DisconnectStart::No,
            "kein zweites Trennen"
        );
        assert!(!monitor.view().can_connect);
        assert!(!monitor.finish_disconnect(&Ok(())));
        monitor.observe(observed("clean", None, false));
        assert_eq!(monitor.display(), Some(&DisplayState::NotConnected));
        assert!(monitor.view().can_connect);
    }

    #[test]
    fn gescheitertes_trennen_zeigt_den_notausgang() {
        let mut monitor = monitor_in("connected", false);
        monitor.begin_disconnect(false);
        assert!(!monitor.finish_disconnect(&Err(Error::Busy)));
        let view = monitor.view();
        assert_eq!(view.message.command, Some(STOP_COMMAND));
        assert!(view.notice.unwrap().text.de().contains("nicht trennen"));
    }

    fn asks(code: &str, blocked: bool) -> CloseDialog {
        match monitor_in(code, blocked).close_decision() {
            CloseDecision::Ask(dialog) => dialog,
            CloseDecision::Close => panic!("{code}: keine Nachfrage"),
        }
    }

    #[test]
    fn nachfrage_bei_verbunden_und_wird_verbunden() {
        for (code, blocked) in [("connected", false), ("connecting", false), ("dns lookup", false)] {
            let dialog = asks(code, blocked);
            assert!(dialog.offers_disconnect, "{code}");
            assert_eq!(dialog.keep_label.de(), "Verbunden lassen und schließen");
        }
    }

    #[test]
    fn keine_nachfrage_bei_nicht_verbunden() {
        assert_eq!(monitor_in("clean", false).close_decision(), CloseDecision::Close);
        assert_eq!(Monitor::new().close_decision(), CloseDecision::Close);
        let mut unreachable = Monitor::new();
        unreachable.observe(Observation::Failed {
            error: Error::NotRunning,
            blocked: false,
        });
        assert_eq!(unreachable.close_decision(), CloseDecision::Close);
    }

    #[test]
    fn hinweis_bei_gesperrtem_internet() {
        for (code, blocked) in [("routed", true), ("connecting", true)] {
            let dialog = asks(code, blocked);
            assert!(
                dialog.text.de().contains("gesperrt") && dialog.offers_disconnect,
                "{code}"
            );
        }
        let mut unreachable = Monitor::new();
        unreachable.observe(Observation::Failed {
            error: Error::NotRunning,
            blocked: true,
        });
        let CloseDecision::Ask(dialog) = unreachable.close_decision() else {
            panic!()
        };
        assert!(!dialog.offers_disconnect);
        assert_eq!(dialog.command, Some(STOP_COMMAND));
    }

    #[test]
    fn trennen_und_schliessen_schliesst_nach_erfolg() {
        let mut monitor = monitor_in("connected", false);
        assert_eq!(monitor.begin_disconnect(true), DisconnectStart::Now);
        assert!(monitor.finish_disconnect(&Ok(())));
    }

    #[test]
    fn trennen_und_schliessen_scheitert_fenster_bleibt_offen() {
        let mut monitor = monitor_in("connected", false);
        monitor.begin_disconnect(true);
        assert!(!monitor.finish_disconnect(&Err(Error::NotRunning)));
        let view = monitor.view();
        assert_eq!(view.message.command, Some(STOP_COMMAND));
        assert!(view.notice.unwrap().text.de().contains("bleibt Celata offen"));
    }

    #[test]
    fn trennen_und_schliessen_waehrend_des_aufbaus() {
        let mut monitor = monitor_in("clean", false);
        monitor.begin_connect("nl.hideservers.net");
        assert_eq!(monitor.begin_disconnect(true), DisconnectStart::Now);
        // The later end of the connection attempt does not overwrite the disconnect.
        monitor.finish_connect(&ConnectResult::FailedReleased(Error::Timeout));
        assert_eq!(monitor.view().notice, None);
        assert!(monitor.finish_disconnect(&Ok(())));
    }

    #[test]
    fn trennen_und_schliessen_ohne_erreichbaren_dienst_geht_nicht() {
        let mut monitor = Monitor::new();
        monitor.observe(Observation::Failed {
            error: Error::NotRunning,
            blocked: true,
        });
        assert_eq!(monitor.begin_disconnect(true), DisconnectStart::No);
        assert_eq!(monitor_in("clean", false).begin_disconnect(true), DisconnectStart::No);
    }

    // --- Unblocking the internet ---

    #[test]
    fn sperre_aufheben_wird_bei_sperre_angeboten() {
        assert!(monitor_in("routed", true).view().can_unblock);
        assert!(monitor_in("dpd timeout", true).view().can_unblock);
        // Automatic reconnection: the block is shown clearly and can be lifted.
        let reconnect = monitor_in("connecting", true);
        assert!(reconnect.view().can_unblock && reconnect.view().message.warning);
        for (code, blocked) in [("clean", false), ("connected", true), ("connecting", false)] {
            assert!(!monitor_in(code, blocked).view().can_unblock, "{code}");
        }
    }

    #[test]
    fn ohne_dienst_bleibt_nur_der_notausgang() {
        let mut monitor = Monitor::new();
        monitor.observe(Observation::Failed {
            error: Error::NotRunning,
            blocked: true,
        });
        assert!(!monitor.view().can_unblock);
        assert!(!monitor.ask_unblock());
        assert_eq!(monitor.view().message.command, Some(STOP_COMMAND));
    }

    #[test]
    fn aufheben_nur_nach_bestaetigung() {
        let mut monitor = monitor_in("routed", true);
        assert!(!monitor.confirm_unblock(), "ohne Nachfrage kein /destroy");
        assert!(monitor.ask_unblock());
        assert!(monitor.view().unblock_asked);
        assert!(monitor.confirm_unblock());
        assert_eq!(monitor.display(), Some(&DisplayState::Disconnecting));
        assert!(!monitor.view().can_unblock, "kein zweites Aufheben");
    }

    #[test]
    fn abbrechen_laesst_die_sperre_bestehen() {
        let mut monitor = monitor_in("routed", true);
        monitor.ask_unblock();
        monitor.cancel_unblock();
        assert!(!monitor.view().unblock_asked);
        assert!(!monitor.confirm_unblock());
        assert_eq!(monitor.display(), Some(&DisplayState::Blocked));
    }

    #[test]
    fn nachfrage_entfaellt_wenn_die_sperre_inzwischen_weg_ist() {
        let mut monitor = monitor_in("connecting", true);
        monitor.ask_unblock();
        monitor.observe(observed("connected", Some("nl.hideservers.net"), true));
        assert!(!monitor.view().unblock_asked);
        assert!(!monitor.confirm_unblock());
    }

    #[test]
    fn aufheben_waehrend_eigenem_aufbau_aus_der_sperre() {
        let mut monitor = monitor_in("routed", true);
        assert_eq!(
            monitor.begin_connect("nl.hideservers.net"),
            Some(Start::Connect { was_free: false })
        );
        assert!(monitor.ask_unblock() && monitor.confirm_unblock());
        monitor.finish_connect(&ConnectResult::FailedBlocked(Error::Timeout));
        monitor.finish_unblock(&Ok(()));
        monitor.observe(observed("clean", None, false));
        assert_eq!(monitor.display(), Some(&DisplayState::NotConnected));
        assert!(monitor.view().notice.unwrap().text.de().contains("aufgehoben"));
    }

    #[test]
    fn gescheitertes_aufheben_zeigt_den_notausgang() {
        let mut monitor = monitor_in("routed", true);
        monitor.ask_unblock();
        monitor.confirm_unblock();
        monitor.finish_unblock(&Err(Error::Busy));
        monitor.observe(observed("routed", None, true));
        let view = monitor.view();
        assert_eq!(view.message.command, Some(STOP_COMMAND));
        assert!(view.notice.unwrap().text.de().contains("nicht aufheben"));
        assert!(view.can_unblock, "ein neuer Versuch bleibt möglich");
    }

    #[test]
    fn warnung_nennt_ungeschuetzten_datenverkehr() {
        assert!(UNBLOCK_WARNING.de().contains("ungeschützt"));
    }

    #[test]
    fn trennen_und_schliessen_loest_aufheben_ab() {
        let mut monitor = monitor_in("routed", true);
        monitor.ask_unblock();
        monitor.confirm_unblock();
        assert_eq!(monitor.begin_disconnect(true), DisconnectStart::Now);
        monitor.finish_unblock(&Err(Error::Busy));
        assert_eq!(monitor.view().notice, None);
        assert!(monitor.finish_disconnect(&Ok(())));
    }

    // --- Final check: every failure names a next step ---

    #[test]
    fn dienst_antwortet_fehlerhaft_nennt_den_neustart() {
        let mut monitor = Monitor::new();
        monitor.observe(Observation::Failed {
            error: Error::Timeout,
            blocked: false,
        });
        let view = monitor.view();
        assert_eq!(view.message.title.de(), "Dienst antwortet nicht richtig");
        assert_eq!(view.message.command, Some(RESTART_COMMAND));

        monitor.observe(Observation::Failed {
            error: Error::NotRunning,
            blocked: false,
        });
        // Not running: connecting starts it.
        assert!(monitor.view().start_on_connect);
        assert_eq!(monitor.view().message.command, None);
        monitor.without_start();
        assert_eq!(monitor.view().message.command, Some(START_COMMAND));
    }

    #[test]
    fn jeder_fehler_des_dienstes_nennt_einen_naechsten_schritt() {
        let errors = [
            Error::NotRunning,
            Error::PermissionDenied,
            Error::Busy,
            Error::Timeout,
            Error::ConnectionLost,
            Error::Status(500),
            Error::InvalidResponse,
            Error::ResponseTooLarge,
            Error::Service("kaputt".to_owned()),
            Error::InvalidHost,
        ];
        for blocked in [false, true] {
            for error in &errors {
                let mut monitor = Monitor::new();
                monitor.observe(Observation::Failed {
                    error: error.clone(),
                    blocked,
                });
                let view = monitor.view();
                // The next step is a command, or connecting, which starts the service first.
                assert!(
                    view.message.command.is_some() || view.start_on_connect,
                    "{error:?}, gesperrt: {blocked}"
                );
                monitor.without_start();
                monitor.observe(Observation::Failed {
                    error: error.clone(),
                    blocked,
                });
                assert!(
                    monitor.view().message.command.is_some(),
                    "{error:?}, gesperrt: {blocked}"
                );
            }
        }
    }

    #[test]
    fn jeder_fehlschlag_einer_aktion_nennt_einen_naechsten_schritt() {
        let error = Error::Timeout;
        for result in [
            ConnectResult::NotStarted(error.clone()),
            ConnectResult::FailedReleased(error.clone()),
            ConnectResult::FailedBlocked(error.clone()),
            ConnectResult::ReleaseFailed(error.clone()),
        ] {
            let notice = result.notice().unwrap();
            let next_step = notice.command.is_some()
                || ["erneut", "noch einmal", "anderen Server"]
                    .iter()
                    .any(|step| notice.text.de().contains(step));
            assert!(next_step, "{result:?}");
        }
        let mut monitor = monitor_in("connected", false);
        monitor.begin_disconnect(false);
        monitor.finish_disconnect(&Err(error.clone()));
        assert_eq!(monitor.view().notice.unwrap().command, Some(STOP_COMMAND));
        let mut monitor = monitor_in("routed", true);
        monitor.ask_unblock();
        monitor.confirm_unblock();
        monitor.finish_unblock(&Err(error));
        assert_eq!(monitor.view().notice.unwrap().command, Some(STOP_COMMAND));
    }

    // --- Switching servers ---

    const NL: &str = "nl.hideservers.net";
    const DE: &str = "de.hideservers.net";

    fn connected_to(host: &str) -> Monitor {
        let mut monitor = Monitor::new();
        monitor.observe(observed("connected", Some(host), true));
        monitor
    }

    fn switching_to(target: &str, back: bool) -> DisplayState {
        DisplayState::Switching {
            target: target.to_owned(),
            back,
        }
    }

    // --- Target while connecting, retarget, inform (feature 013) ---

    const BERLIN: &str = "berlin.hideservers.net";

    #[test]
    fn verbinden_nennt_das_ziel() {
        let mut monitor = monitor_in("clean", false);
        assert_eq!(monitor.view().connect_target, None);
        assert_eq!(monitor.begin_connect(BERLIN), Some(Start::Connect { was_free: true }));
        let view = monitor.view();
        assert_eq!(view.connect_target.as_deref(), Some(BERLIN));
        assert_eq!(view.message.title, Key::StateConnectingToTitle);
        assert_eq!(view.message.title.de(), "Verbinde mit {0} …");
        assert_eq!(view.switch_target, None);
        // Intermediate states of the service keep the target.
        monitor.observe(observed("connecting", None, true));
        assert_eq!(monitor.view().connect_target.as_deref(), Some(BERLIN));
        // After the result the target is gone.
        monitor.finish_connect(&ConnectResult::Connected);
        monitor.observe(observed("connected", Some(BERLIN), true));
        let view = monitor.view();
        assert_eq!(view.connect_target, None);
        assert_eq!(view.message.title, Key::StateConnectedTitle);
    }

    #[test]
    fn verbinden_aus_der_sperre_nennt_das_ziel() {
        let mut monitor = monitor_in("routed", true);
        assert_eq!(monitor.begin_connect(BERLIN), Some(Start::Connect { was_free: false }));
        let view = monitor.view();
        assert_eq!(view.connect_target.as_deref(), Some(BERLIN));
        assert_eq!(view.message.title, Key::StateConnectingBlockedToTitle);
        assert!(view.message.warning);
        assert_eq!(
            crate::i18n::text(Language::En, view.message.title),
            "Connecting to {0}, internet blocked"
        );
    }

    #[test]
    fn ohne_eigenes_verbinden_bleiben_die_bisherigen_titel() {
        for (code, blocked, title) in [
            ("connecting", false, Key::StateConnectingTitle),
            ("connecting", true, Key::StateConnectingBlockedTitle),
        ] {
            let monitor = monitor_in(code, blocked);
            let view = monitor.view();
            assert_eq!(view.connect_target, None, "{code}");
            assert_eq!(view.message.title, title, "{code}");
        }
        // A switch keeps its own title.
        let mut switching = connected_to(NL);
        switching.begin_connect(DE).unwrap();
        assert_eq!(switching.view().connect_target, None);
        assert_eq!(switching.view().message.title, Key::StateSwitchingTitle);
    }

    #[test]
    fn neues_ziel_nur_vor_dem_ersten_schritt_des_wechsels() {
        let mut monitor = connected_to(NL);
        monitor.begin_connect(DE).unwrap();
        // Invalid, connected host or "best": no.
        for host in ["best", "", NL, "NL.hideservers.net", "x.example.org"] {
            assert!(!monitor.retarget(host), "{host}");
        }
        assert!(monitor.retarget(BERLIN));
        assert_eq!(monitor.display(), Some(&switching_to(BERLIN, false)));
        assert_eq!(monitor.view().switch_target.as_deref(), Some(BERLIN));
        // After the first step at the service the target is fixed.
        assert!(monitor.switch_proceed(Phase::Forward));
        assert!(!monitor.retarget(DE));
        monitor.finish_switch(&SwitchResult::Switched);
        assert_eq!(monitor.view().connected_host, BERLIN);
        // Not during a connection attempt, not without an operation, not while reconnecting.
        let mut connecting = monitor_in("clean", false);
        assert!(!connecting.retarget(BERLIN));
        connecting.begin_connect(DE).unwrap();
        assert!(!connecting.retarget(BERLIN));
        let mut reapplying = connected_to(NL);
        reapplying.begin_reapply().unwrap();
        assert!(!reapplying.retarget(BERLIN));
        // Not after "Disconnect" was requested.
        let mut stopping = connected_to(NL);
        stopping.begin_connect(DE).unwrap();
        assert_eq!(stopping.begin_disconnect(false), DisconnectStart::Deferred);
        assert!(!stopping.retarget(BERLIN));
    }

    #[test]
    fn wechsel_endet_ohne_aufruf_beim_verbundenen_server() {
        let mut monitor = connected_to(NL);
        monitor.begin_connect(DE).unwrap();
        assert!(monitor.abandon_switch(Phrase::key(Key::NoticeBestAlready)));
        let view = monitor.view();
        assert_eq!(view.connected_host, NL);
        assert!(!view.switching && view.can_switch);
        assert_eq!(view.notice.unwrap().text, Phrase::key(Key::NoticeBestAlready));
        // Only once, and not after the first step, after "Disconnect" or while reconnecting.
        assert!(!monitor.abandon_switch(Phrase::key(Key::NoticeBestAlready)));
        let mut called = connected_to(NL);
        called.begin_connect(DE).unwrap();
        assert!(called.switch_proceed(Phase::Forward));
        assert!(!called.abandon_switch(Phrase::key(Key::NoticeBestAlready)));
        let mut stopping = connected_to(NL);
        stopping.begin_connect(DE).unwrap();
        stopping.begin_disconnect(false);
        assert!(!stopping.abandon_switch(Phrase::key(Key::NoticeBestAlready)));
        let mut reapplying = connected_to(NL);
        reapplying.begin_reapply().unwrap();
        assert!(!reapplying.abandon_switch(Phrase::key(Key::NoticeBestAlready)));
        assert!(!monitor_in("clean", false).abandon_switch(Phrase::key(Key::NoticeBestAlready)));
    }

    #[test]
    fn jede_gesetzte_meldung_zaehlt_auch_mit_gleichem_text() {
        let mut monitor = connected_to(BERLIN);
        let start = monitor.notice_count();
        assert!(monitor.inform(Phrase::key(Key::NoticeBestAlready)));
        assert_eq!(monitor.notice_count(), start + 1);
        assert!(monitor.inform(Phrase::key(Key::NoticeBestAlready)));
        assert_eq!(monitor.notice_count(), start + 2);
        // Queries of `/state` and removing the notice do not count.
        monitor.observe(observed("connected", Some(BERLIN), true));
        monitor.begin_connect(NL).unwrap();
        assert_eq!(monitor.view().notice, None);
        assert_eq!(monitor.notice_count(), start + 2);
        // A refused attempt counts as soon as its message is set.
        let mut free = monitor_in("clean", false);
        let before = free.notice_count();
        free.begin_connect(DE).unwrap();
        free.finish_connect(&ConnectResult::NotStarted(Error::NotRunning));
        assert!(free.view().notice.is_some());
        assert_eq!(free.notice_count(), before + 1);
    }

    #[test]
    fn hinweis_ohne_vorgang() {
        let mut monitor = connected_to(BERLIN);
        assert!(monitor.inform(Phrase::key(Key::NoticeBestAlready)));
        let notice = monitor.view().notice.expect("Hinweis");
        assert_eq!(notice.text, Phrase::key(Key::NoticeBestAlready));
        assert_eq!(notice.command, None);
        // The state stays as it is.
        assert_eq!(monitor.view().connected_host, BERLIN);
        // A query of `/state` keeps the notice.
        monitor.observe(observed("connected", Some(BERLIN), true));
        assert!(monitor.view().notice.is_some());
        // The next operation removes it.
        monitor.begin_connect(NL).unwrap();
        assert_eq!(monitor.view().notice, None);
        // During an operation no notice.
        assert!(!monitor.inform(Phrase::key(Key::NoticeBestUnknown)));
        assert_eq!(monitor.view().notice, None);
        // Also without a connection and while blocked.
        for (code, blocked) in [("clean", false), ("routed", true)] {
            let mut monitor = monitor_in(code, blocked);
            assert!(monitor.inform(Phrase::key(Key::NoticeBestUnknown)), "{code}");
            assert!(monitor.view().notice.is_some(), "{code}");
            assert!(monitor.view().can_connect, "{code}");
        }
    }

    #[test]
    fn wechsel_startet_nur_aus_verbunden_mit_bekanntem_server() {
        let mut monitor = connected_to(NL);
        assert!(monitor.view().can_switch);
        assert_eq!(monitor.view().connected_host, NL);
        assert_eq!(monitor.begin_connect(DE), Some(Start::Switch { from: NL.to_owned() }));
        assert_eq!(monitor.display(), Some(&switching_to(DE, false)));
        let view = monitor.view();
        assert!(view.switching && !view.can_switch && !view.can_connect && !view.can_unblock);
        assert_eq!(view.switch_target.as_deref(), Some(DE));
        assert_eq!(view.message.title.de(), "Wechsel zu {0} …");
        assert!(!view.message.warning, "die gewollte kurze Sperre ist keine Warnung");

        let mut unknown = Monitor::new();
        unknown.observe(observed("connected", None, true));
        assert!(!unknown.view().can_switch);
        assert_eq!(unknown.begin_connect(DE), None);
        for (code, blocked) in [("connecting", false), ("disconnecting", true)] {
            assert_eq!(monitor_in(code, blocked).begin_connect(DE), None, "{code}");
        }
    }

    #[test]
    fn gleicher_server_startet_keinen_wechsel() {
        // The same server, also in a different spelling
        let mut monitor = connected_to(NL);
        assert_eq!(monitor.begin_connect(NL), None);
        assert_eq!(monitor.begin_connect("NL.hideservers.net"), None);
        assert_eq!(
            monitor.display(),
            Some(&DisplayState::Connected {
                server: Some(NL.to_owned())
            })
        );
    }

    #[test]
    fn ungueltiger_host_startet_keinen_wechsel() {
        let mut monitor = connected_to(NL);
        assert_eq!(monitor.begin_connect("boese.example.org"), None);
        assert_eq!(monitor.begin_connect(""), None);
        assert!(monitor.view().can_switch);
    }

    #[test]
    fn zweiter_start_waehrend_des_wechsels_bewirkt_nichts() {
        // A second request while a switch is running is ignored
        let mut monitor = connected_to(NL);
        monitor.begin_connect(DE);
        assert_eq!(monitor.begin_connect("fr.hideservers.net"), None);
        assert_eq!(monitor.display(), Some(&switching_to(DE, false)));
    }

    #[test]
    fn zwischenstaende_zeigen_den_wechsel_und_keine_sperre() {
        let mut monitor = connected_to(NL);
        monitor.begin_connect(DE);
        for (code, host, blocked) in [
            ("disconnecting", Some(NL), true),
            ("routed", Some(NL), true),
            ("connecting", Some(DE), true),
            ("dns lookup", None, true),
            ("connected", Some(NL), true),
        ] {
            monitor.observe(observed(code, host, blocked));
            assert_eq!(monitor.display(), Some(&switching_to(DE, false)), "{code}");
            assert!(!monitor.view().message.warning, "{code}");
        }
        monitor.observe(observed("connected", Some(DE), true));
        assert_eq!(
            monitor.display(),
            Some(&DisplayState::Connected {
                server: Some(DE.to_owned())
            })
        );
    }

    #[test]
    fn nach_gelungenem_wechsel_kein_hinweis() {
        let mut monitor = connected_to(NL);
        monitor.begin_connect(DE);
        monitor.finish_switch(&SwitchResult::Switched);
        let view = monitor.view();
        assert_eq!(view.notice, None);
        assert_eq!(view.server.as_deref(), Some(DE));
        assert!(view.can_switch && !view.switching);
        // The confirmation by `/state` changes nothing any more.
        monitor.observe(observed("connected", Some(DE), true));
        assert_eq!(monitor.view().notice, None);
    }

    #[test]
    fn abgelehnter_wechsel_bleibt_verbunden_mit_hinweis() {
        for (result, expected) in [
            (SwitchResult::Refused, "Wechsel nicht möglich"),
            (SwitchResult::Unreachable, NL),
        ] {
            let mut monitor = connected_to(NL);
            monitor.begin_connect(DE);
            monitor.finish_switch(&result);
            assert_eq!(
                monitor.display(),
                Some(&DisplayState::Connected {
                    server: Some(NL.to_owned())
                })
            );
            // The notice survives the confirmation by `/state`.
            monitor.observe(observed("connected", Some(NL), true));
            let notice = monitor.view().notice.expect("Hinweis");
            assert!(notice.text.de().contains(expected), "{result:?}: {}", notice.text.de());
            assert!(!notice.text.de().contains(FAKE_SECRET_STATE));
        }
    }

    /// Invented value that must never show up in notices.
    const FAKE_SECRET_STATE: &str = "ERFUNDENER-TESTWERT-KEIN-ECHTER-SCHLUESSEL";

    #[test]
    fn gescheiterter_wechsel_zeigt_die_sperre_mit_naechsten_schritten() {
        let mut monitor = connected_to(NL);
        monitor.begin_connect(DE);
        monitor.finish_switch(&SwitchResult::Stranded(Error::Service("dial failed".to_owned())));
        monitor.observe(observed("routed", Some(DE), true));
        let view = monitor.view();
        assert_eq!(monitor.display(), Some(&DisplayState::Blocked));
        assert!(view.can_unblock && view.can_connect);
        assert!(view.notice.unwrap().text.de().contains("hebe die Sperre auf"));
    }

    #[test]
    fn ohne_dienst_kein_irrefuehrender_hinweis() {
        let mut monitor = connected_to(NL);
        monitor.begin_connect(DE);
        monitor.observe(Observation::Failed {
            error: Error::NotRunning,
            blocked: false,
        });
        monitor.finish_switch(&SwitchResult::Stranded(Error::NotRunning));
        assert_eq!(monitor.view().notice, None);
        assert_eq!(monitor.display(), Some(&DisplayState::ServiceUnreachable));
    }

    #[test]
    fn trennen_waehrend_des_wechsels_uebernimmt_der_wechsel_thread() {
        let mut monitor = connected_to(NL);
        monitor.begin_connect(DE);
        assert!(monitor.view().can_disconnect);
        assert_eq!(monitor.begin_disconnect(false), DisconnectStart::Deferred);
        assert_eq!(monitor.display(), Some(&DisplayState::Disconnecting));
        assert!(!monitor.view().can_disconnect);
        assert_eq!(
            monitor.begin_disconnect(false),
            DisconnectStart::No,
            "kein zweites Trennen"
        );
        // Intermediate states now show "Disconnecting …".
        monitor.observe(observed("routed", Some(NL), true));
        assert_eq!(monitor.display(), Some(&DisplayState::Disconnecting));
        // The thread stops, the action turns into a disconnect.
        assert!(!monitor.switch_proceed(Phase::Forward));
        assert!(
            !monitor.finish_disconnect(&Ok(())),
            "ohne Schließen bleibt das Fenster offen"
        );
        monitor.observe(observed("clean", None, false));
        assert_eq!(monitor.display(), Some(&DisplayState::NotConnected));
    }

    #[test]
    fn trennen_und_schliessen_waehrend_des_wechsels() {
        let mut monitor = connected_to(NL);
        monitor.begin_connect(DE);
        // The same question as during a connection attempt
        let CloseDecision::Ask(dialog) = monitor.close_decision() else {
            panic!("Nachfrage erwartet")
        };
        assert!(dialog.offers_disconnect);
        assert_eq!(dialog.keep_label.de(), "Verbunden lassen und schließen");
        // A "Disconnect" that was already requested can still be extended to closing.
        assert_eq!(monitor.begin_disconnect(false), DisconnectStart::Deferred);
        assert_eq!(monitor.begin_disconnect(true), DisconnectStart::Deferred);
        assert_eq!(monitor.begin_disconnect(true), DisconnectStart::No);
        assert!(!monitor.switch_proceed(Phase::Forward));
        assert!(monitor.finish_disconnect(&Ok(())));
    }

    #[test]
    fn spaetes_ende_des_wechsels_nach_trennen_aendert_nichts() {
        let mut monitor = connected_to(NL);
        monitor.begin_connect(DE);
        monitor.begin_disconnect(false);
        assert!(!monitor.switch_proceed(Phase::Forward));
        monitor.finish_switch(&SwitchResult::Switched);
        assert_eq!(monitor.display(), Some(&DisplayState::Disconnecting));
        assert_eq!(monitor.view().notice, None);
    }

    #[test]
    fn rueckweg_zeigt_zurueck_zu_mit_dem_alten_server() {
        let mut monitor = connected_to(NL);
        monitor.begin_connect(DE);
        assert!(monitor.switch_proceed(Phase::Forward));
        assert!(monitor.switch_proceed(Phase::Back));
        assert_eq!(monitor.display(), Some(&switching_to(NL, true)));
        let view = monitor.view();
        assert_eq!(view.message.title.de(), "Zurück zu {0} …");
        assert_eq!(view.switch_target.as_deref(), Some(NL));
        // Intermediate states keep showing the way back, "Connected" only with the previous server.
        monitor.observe(observed("connecting", Some(NL), true));
        assert_eq!(monitor.display(), Some(&switching_to(NL, true)));
        monitor.observe(observed("connected", Some(NL), true));
        assert_eq!(
            monitor.display(),
            Some(&DisplayState::Connected {
                server: Some(NL.to_owned())
            })
        );
    }

    #[test]
    fn nach_rueckweg_verbunden_mit_hinweis_auf_beide_server() {
        let mut monitor = connected_to(NL);
        monitor.begin_connect(DE);
        monitor.switch_proceed(Phase::Back);
        monitor.observe(observed("connected", Some(NL), true));
        monitor.finish_switch(&SwitchResult::Returned(Error::Service("dial failed".to_owned())));
        monitor.observe(observed("connected", Some(NL), true));
        let view = monitor.view();
        assert_eq!(view.server.as_deref(), Some(NL));
        assert!(view.can_switch);
        let text = view.notice.expect("Hinweis").text.de();
        assert!(text.contains(NL) && text.contains(DE), "{text}");
    }

    #[test]
    fn ohne_dienst_nach_gescheitertem_rueckweg_kein_hinweis() {
        let mut monitor = connected_to(NL);
        monitor.begin_connect(DE);
        monitor.finish_switch(&SwitchResult::Stranded(Error::PermissionDenied));
        assert_eq!(monitor.view().notice, None);
    }

    #[test]
    fn bekannter_ablauf_ohne_wechsel_bleibt_unveraendert() {
        // From "Internet blocked" a double click connects as before; that is not a switch.
        let mut monitor = monitor_in("routed", true);
        assert_eq!(monitor.begin_connect(DE), Some(Start::Connect { was_free: false }));
        assert!(!monitor.view().switching);
    }

    // --- Reconnecting after a change of settings ---

    fn connected_to_nl() -> Monitor {
        let mut monitor = Monitor::new();
        monitor.observe(observed("connected", Some("nl.hideservers.net"), true));
        monitor
    }

    #[test]
    fn neu_verbinden_nur_bei_verbindung_ohne_vorgang() {
        assert_eq!(
            connected_to_nl().reapply_gate(),
            ReapplyGate::Now("nl.hideservers.net".to_owned())
        );
        // Blocked or free: only save, applies from the next connection.
        assert_eq!(monitor_in("routed", true).reapply_gate(), ReapplyGate::Never);
        assert_eq!(monitor_in("clean", false).reapply_gate(), ReapplyGate::Never);
        assert_eq!(Monitor::new().reapply_gate(), ReapplyGate::Never);
        assert_eq!(monitor_in("routed", true).begin_reapply(), None);
        // A switch is running: the bundle waits.
        let mut switching = connected_to_nl();
        switching.begin_connect("de.hideservers.net").unwrap();
        assert_eq!(switching.reapply_gate(), ReapplyGate::Wait);
        assert_eq!(switching.begin_reapply(), None);
        // An own connection attempt is running: wait as well.
        let mut connecting = monitor_in("clean", false);
        connecting.begin_connect("nl.hideservers.net").unwrap();
        assert_eq!(connecting.reapply_gate(), ReapplyGate::Wait);
    }

    #[test]
    fn neu_verbinden_zeigt_einstellungen_werden_uebernommen() {
        let mut monitor = connected_to_nl();
        assert_eq!(monitor.begin_reapply().as_deref(), Some("nl.hideservers.net"));
        assert_eq!(monitor.display(), Some(&DisplayState::Reapplying { back: false }));
        assert_eq!(monitor.view().message.title.de(), "Einstellungen werden übernommen …");
        assert!(monitor.view().switching);
        assert!(monitor.view().can_disconnect);
        // Intermediate states of the service, even "connected" with the same host, do not change the display.
        for code in ["connected", "disconnecting", "routed", "connecting"] {
            monitor.observe(observed(code, Some("nl.hideservers.net"), true));
            assert_eq!(
                monitor.display(),
                Some(&DisplayState::Reapplying { back: false }),
                "{code}"
            );
        }
        assert!(monitor.switch_proceed(Phase::Back));
        assert_eq!(
            monitor.view().message.title.de(),
            "Vorherige Einstellungen werden wiederhergestellt …"
        );
        assert_eq!(
            close_decision(monitor.display()),
            close_decision(Some(&DisplayState::Connecting))
        );
        assert_eq!(
            crate::look::main_button(monitor.display(), &monitor.view()).label.de(),
            "Wird neu verbunden …"
        );
    }

    #[test]
    fn nach_dem_neu_verbinden() {
        let mut monitor = connected_to_nl();
        monitor.begin_reapply();
        monitor.finish_switch(&SwitchResult::Switched);
        assert_eq!(
            monitor.display(),
            Some(&DisplayState::Connected {
                server: Some("nl.hideservers.net".to_owned())
            })
        );
        assert_eq!(monitor.view().notice, None);
        assert_eq!(reapply_notice(&SwitchResult::Switched), Some(Phrase::key(REAPPLIED)));

        let mut monitor = connected_to_nl();
        monitor.begin_reapply();
        monitor.finish_switch(&SwitchResult::Returned(Error::Timeout));
        let notice = monitor.view().notice.unwrap().text.de();
        assert!(
            notice.starts_with("Neue Einstellungen nicht übernommen, Verbindung mit den vorherigen"),
            "{notice}"
        );
        assert!(matches!(monitor.display(), Some(DisplayState::Connected { .. })));

        let mut monitor = connected_to_nl();
        monitor.begin_reapply();
        monitor.finish_switch(&SwitchResult::Rejected("bad".to_owned()));
        assert!(monitor.view().notice.unwrap().text.de().contains("abgelehnt (bad)"));
        assert!(matches!(monitor.display(), Some(DisplayState::Connected { .. })));
    }

    #[test]
    fn trennen_waehrend_des_neu_verbindens() {
        let mut monitor = connected_to_nl();
        monitor.begin_reapply();
        assert_eq!(monitor.begin_disconnect(false), DisconnectStart::Deferred);
        assert!(!monitor.switch_proceed(Phase::Forward));
        assert_eq!(monitor.display(), Some(&DisplayState::Disconnecting));
    }

    #[test]
    fn jedes_ergebnis_hat_eine_meldung() {
        let results = [
            SwitchResult::Returned(Error::Timeout),
            SwitchResult::Stranded(Error::Timeout),
            SwitchResult::Refused,
            SwitchResult::Unreachable,
            SwitchResult::NotStarted(Error::Busy),
            SwitchResult::Unclear(Error::Timeout),
            SwitchResult::Rejected("x".to_owned()),
            SwitchResult::Untested("0.9.15".to_owned()),
        ];
        for result in results {
            assert!(
                reapply_notice(&result).is_some_and(|text| !text.de().is_empty()),
                "{result:?}"
            );
        }
        assert_eq!(reapply_notice(&SwitchResult::Stopped(Ok(()))), None);
    }

    // --- Connection details and public address ---

    fn details_json(code: &str) -> ServiceState {
        serde_json::from_str(&format!(
            r#"{{"code":"{code}","host":"nl.hideservers.net","rx":5,"tx":6,"timestamp":"2026-11-09T12:00:00Z","allowedIps":["10.0.0.2"],"DNS":["10.0.0.1"],"publicKey":"GEHEIM-SCHLUESSEL-123","presharedKey":"GEHEIM-SCHLUESSEL-123","sessionToken":"GEHEIM-SCHLUESSEL-123","endpoint":"203.0.113.1:432","gateway":"10.0.0.1"}}"#
        ))
        .unwrap()
    }

    #[test]
    fn schluessel_landen_in_keinem_typ() {
        let state = details_json("connected");
        assert_eq!((state.rx, state.tx), (Some(5), Some(6)));
        assert_eq!(state.addresses, ["10.0.0.2"]);
        let mut monitor = Monitor::new();
        monitor.observe(Observation::State {
            state: state.clone(),
            blocked: true,
        });
        let shown = format!("{state:?} {:?} {:?}", monitor, monitor.view());
        assert!(!shown.contains("GEHEIM-SCHLUESSEL-123"));
        assert!(!shown.contains("203.0.113.1"));
    }

    #[test]
    fn details_nur_bei_verbunden_im_monitor() {
        let mut monitor = Monitor::new();
        monitor.observe(Observation::State {
            state: details_json("connected"),
            blocked: true,
        });
        assert_eq!(monitor.view().service.and_then(|state| state.rx), Some(5));
        monitor.observe(observed("routed", None, true));
        assert_eq!(monitor.view().service, None);
    }

    fn external(v4: &str) -> Observation {
        let state = ServiceState {
            code: "external ips".to_owned(),
            external_v4: Some(v4.to_owned()),
            ..ServiceState::default()
        };
        Observation::State { state, blocked: false }
    }

    #[test]
    fn oeffentliche_adresse_von_selbst_aber_nie_bei_sperre() {
        let now = Instant::now();
        let mut monitor = monitor_in("clean", false);
        // A report without a query changes nothing.
        monitor.observe(external("203.0.113.7"));
        assert_eq!(monitor.view().external_ip, ExternalIp::Idle);
        assert!(monitor.begin_external_ip(now));
        assert!(!monitor.begin_external_ip(now), "nur einmal je Zustand");
        let (_, changed) = monitor.observe(external("203.0.113.7"));
        assert!(changed);
        assert_eq!(monitor.view().external_ip.column(false, Language::De), "203.0.113.7");
        assert!(!monitor.begin_external_ip(now + std::time::Duration::from_secs(60)));
        // A change of state discards the result; nothing is queried while blocked.
        monitor.observe(observed("routed", None, true));
        assert_eq!(monitor.view().external_ip, ExternalIp::Idle);
        assert!(!monitor.begin_external_ip(now));
        // Connected: query again; limit of 10 seconds, again after 30 seconds.
        monitor.observe(observed("connected", Some("nl.hideservers.net"), true));
        assert!(monitor.begin_external_ip(now));
        assert!(monitor.expire_external_ip(now + std::time::Duration::from_secs(10)));
        assert!(matches!(monitor.view().external_ip, ExternalIp::Failed { .. }));
        assert!(!monitor.begin_external_ip(now + std::time::Duration::from_secs(20)));
        assert!(monitor.begin_external_ip(now + std::time::Duration::from_secs(40)));
        // Not without a reachable service or during an operation.
        assert!(!Monitor::new().begin_external_ip(now));
        let mut busy = monitor_in("clean", false);
        busy.begin_connect("nl.hideservers.net");
        assert!(!busy.begin_external_ip(now));
    }

    // --- Blocking the internet ---

    #[test]
    fn sperren_nur_wenn_nicht_verbunden_und_sperre_an() {
        let mut free = monitor_in("clean", false);
        assert!(free.can_route(true));
        assert!(!free.can_route(false));
        assert!(!monitor_in("routed", true).can_route(true));
        assert!(!connected_to_nl().can_route(true));
        assert!(!Monitor::new().can_route(true));
        assert!(free.begin_route(true));
        assert!(!free.can_route(true), "nicht zweimal");
        assert!(!free.view().can_connect, "kein Verbinden während des Sperrens");
        free.finish_route(&RouteResult::Failed(Error::Busy));
        assert!(
            free.view()
                .notice
                .unwrap()
                .text
                .de()
                .starts_with("Das Internet ließ sich nicht sperren")
        );
        assert!(free.begin_route(true));
        free.finish_route(&RouteResult::Routed);
        assert_eq!(free.view().notice, None);
        free.observe(observed("routed", None, true));
        assert_eq!(free.display(), Some(&DisplayState::Blocked));
    }

    // --- Blocking the internet with the main button ---

    #[test]
    fn laufendes_sperren_wird_gemeldet() {
        assert!(!Monitor::new().view().routing);
        let mut monitor = monitor_in("clean", false);
        assert!(!monitor.view().routing);
        assert!(monitor.begin_route(true));
        assert!(monitor.view().routing);
        monitor.finish_route(&RouteResult::Routed);
        assert!(!monitor.view().routing);
        assert!(monitor.begin_route(true));
        monitor.finish_route(&RouteResult::Rejected("abgelehnt".to_owned()));
        assert!(
            !monitor.view().routing,
            "nach einer Ablehnung steht der Knopf wieder in der Ausgangslage"
        );
    }

    #[test]
    fn statuskarte_verweist_bei_sperre_auf_das_rote_feld() {
        const HINWEIS: &str = "Zum Aufheben klickst du auf das rote Feld rechts im Knopf unten.";
        for display in [DisplayState::Blocked, DisplayState::ConnectingBlocked] {
            let message = display.message();
            assert!(message.text.de().ends_with(HINWEIS), "{display:?}");
            assert!(message.text.de().contains("hebe die Sperre auf"), "{display:?}");
            assert!(message.warning);
        }
        // Without the service the button does not help: the command for the terminal remains.
        let offline = DisplayState::ServiceUnreachableBlocked.message();
        assert_eq!(offline.command, Some(STOP_COMMAND));
        assert!(!offline.text.de().contains("rote Feld"));
    }

    #[test]
    fn veralteter_klick_auf_aufheben_bewirkt_nichts() {
        assert!(!monitor_in("clean", false).ask_unblock(), "nichts gesperrt");
        assert!(!connected_to_nl().ask_unblock(), "verbunden");
        let mut blocked = monitor_in("routed", true);
        assert!(!blocked.confirm_unblock(), "ohne Nachfrage kein Aufheben");
    }

    #[test]
    fn veralteter_klick_auf_sperren_bewirkt_nichts() {
        assert!(!monitor_in("routed", true).begin_route(true), "schon gesperrt");
        assert!(!connected_to_nl().begin_route(true), "verbunden");
        let mut free = monitor_in("clean", false);
        assert!(!free.begin_route(false), "Internet-Sperre in den Einstellungen aus");
        assert!(free.begin_route(true));
        assert!(!free.begin_route(true), "nicht zweimal");
    }

    /// Feature 012: only results in which the service tried the name count as a failed name.
    #[test]
    fn nur_versuchte_namen_gelten_als_gescheitert() {
        let failed = [
            ConnectResult::FailedReleased(Error::Timeout),
            ConnectResult::FailedBlocked(Error::Timeout),
            ConnectResult::ReleaseFailed(Error::Timeout),
        ];
        for result in &failed {
            assert!(result.name_failed(), "{result:?}");
        }
        let not_tried = [
            ConnectResult::Connected,
            ConnectResult::NotStarted(Error::Timeout),
            ConnectResult::Rejected("x".to_owned()),
            ConnectResult::Untested("0.9.15".to_owned()),
        ];
        for result in &not_tried {
            assert!(!result.name_failed(), "{result:?}");
        }
    }
}
