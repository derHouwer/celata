// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! Starts the hide.me service through systemd when Celata needs it; never stops, restarts, enables or
//! disables it.
//!
//! Every decision is a pure function at the top of this module: whether to start, what an answer of systemd
//! means, whether a running start is ready or failed, and the command that sets up the rule for polkit. Only
//! the functions at the bottom talk to systemd, over the system bus, one connection per operation and without
//! signals. Whether a start without a password is allowed is decided by polkit after the rule
//! `packaging/polkit/49-celata.rules`; Celata never runs `sudo`, a shell or another program for it.
//!
//! Read are only `/proc/self/status` and `/etc/group`; no network, no file is written.

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use zbus::Message;
use zbus::blocking::Connection;
use zbus::zvariant::{DynamicType, OwnedObjectPath, OwnedValue, Structure};

/// Group whose members may use the control socket and start the service.
pub const GROUP: &str = "celata";

/// The rule for polkit, built in; `celata --polkit-rule` prints it.
pub const RULE: &str = include_str!("../packaging/polkit/49-celata.rules");

/// Where the setup by hand puts the rule (the packages put it below `/usr/share`).
pub const RULE_PATH: &str = "/etc/polkit-1/rules.d/49-celata.rules";

/// Longest wait for the service after a start was accepted: covers the wait for the network
/// (`network-online.target`, at most 60 seconds with `NetworkManager`) and the start itself.
pub const START_WAIT: Duration = Duration::from_secs(90);

/// Time limit of a call without a password prompt.
const BUS_TIMEOUT: Duration = Duration::from_secs(25);
/// Time limit of the start with a password prompt: the user has to type the password.
const PROMPT_TIMEOUT: Duration = Duration::from_secs(300);

/// polkit wants a password, but the call did not allow a prompt.
pub const ERROR_INTERACTIVE: &str = "org.freedesktop.DBus.Error.InteractiveAuthorizationRequired";
/// polkit refused: no rule, or the prompt was cancelled or the password was wrong.
pub const ERROR_ACCESS_DENIED: &str = "org.freedesktop.DBus.Error.AccessDenied";
/// systemd does not know the unit.
pub const ERROR_NO_SUCH_UNIT: &str = "org.freedesktop.systemd1.NoSuchUnit";
/// The unit is masked.
pub const ERROR_UNIT_MASKED: &str = "org.freedesktop.systemd1.UnitMasked";

// ---------------------------------------------------------------------------------------------------
// Group
// ---------------------------------------------------------------------------------------------------

/// Is this process in the group, as it counts for the control socket?
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GroupCheck {
    Member,
    /// The number of the group is known and missing from the groups of the process (for example added to the
    /// group, but not logged in anew since).
    NotMember,
    /// Not decidable: the group is not in `/etc/group` (directory services), or a file could not be read.
    #[default]
    Unknown,
}

/// Decides from the text of `/proc/self/status` and `/etc/group`. Only the exact name counts; an empty text
/// (file not readable) gives `Unknown`.
pub fn group_check(proc_status: &str, etc_group: &str, group: &str) -> GroupCheck {
    let number = etc_group.lines().find_map(|line| {
        let mut fields = line.split(':');
        (fields.next()? == group).then_some(())?;
        fields.next()?;
        fields.next()?.trim().parse::<u32>().ok()
    });
    let Some(number) = number else {
        return GroupCheck::Unknown;
    };
    let Some(groups) = proc_status.lines().find_map(|line| line.strip_prefix("Groups:")) else {
        return GroupCheck::Unknown;
    };
    if groups.split_whitespace().any(|gid| gid.parse::<u32>() == Ok(number)) {
        GroupCheck::Member
    } else {
        GroupCheck::NotMember
    }
}

/// Reads `/proc/self/status` and `/etc/group`. A file that cannot be read gives an empty text, never an error.
pub fn read_group_inputs() -> (String, String) {
    let read = |path: &str| std::fs::read_to_string(path).unwrap_or_default();
    (read("/proc/self/status"), read("/etc/group"))
}

// ---------------------------------------------------------------------------------------------------
// State of the unit and the decision before a start
// ---------------------------------------------------------------------------------------------------

/// `LoadState` of the unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadState {
    Loaded,
    NotFound,
    Masked,
    Other,
}

impl LoadState {
    /// An unknown text becomes `Other`, never an error.
    pub fn parse(text: &str) -> Self {
        match text {
            "loaded" => Self::Loaded,
            "not-found" => Self::NotFound,
            "masked" => Self::Masked,
            _ => Self::Other,
        }
    }
}

/// `ActiveState` of the unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveState {
    Active,
    Activating,
    Deactivating,
    Reloading,
    Inactive,
    Failed,
    Other,
}

impl ActiveState {
    /// An unknown text becomes `Other`, never an error.
    pub fn parse(text: &str) -> Self {
        match text {
            "active" => Self::Active,
            "activating" => Self::Activating,
            "deactivating" => Self::Deactivating,
            "reloading" => Self::Reloading,
            "inactive" => Self::Inactive,
            "failed" => Self::Failed,
            _ => Self::Other,
        }
    }
}

/// What systemd says about the unit; readable without any rights.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitInfo {
    pub load: LoadState,
    pub active: ActiveState,
    /// `SubState`, for example `running` or `auto-restart`.
    pub sub: String,
    /// `Result` of the service, for example `success` or `exit-code`; empty if unknown.
    pub result: String,
    /// A job for the unit is queued or running (property `Job` other than `(0, "/")`).
    pub job: bool,
}

/// The system bus or systemd did not answer as expected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusError {
    /// No system bus.
    NoBus,
    /// No answer within the time limit, or an error answer.
    NoAnswer,
    /// An answer of another form than expected.
    Unexpected,
}

/// What to do before a start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Before {
    /// Call `StartUnit`.
    Start,
    /// A job runs or the unit is starting (for example the start at boot waits for the network): only wait.
    Wait,
    /// The unit is active: start nothing; the control socket decides as before.
    Running,
    /// The unit does not exist or is masked.
    NotInstalled,
    /// The process is not in the group: the socket could not be used anyway.
    NoPermission,
    /// systemd could not be asked.
    NoSystemBus,
}

/// Decides in this order: bus, not installed, group, active, starting or stopping, start. A running stop is
/// waited for, never replaced.
pub fn before_start(unit: Result<&UnitInfo, &BusError>, group: GroupCheck) -> Before {
    let Ok(unit) = unit else {
        return Before::NoSystemBus;
    };
    if matches!(unit.load, LoadState::NotFound | LoadState::Masked) {
        return Before::NotInstalled;
    }
    if group == GroupCheck::NotMember {
        return Before::NoPermission;
    }
    match unit.active {
        ActiveState::Active | ActiveState::Reloading => Before::Running,
        ActiveState::Activating | ActiveState::Deactivating | ActiveState::Other => Before::Wait,
        ActiveState::Inactive | ActiveState::Failed if unit.job => Before::Wait,
        ActiveState::Inactive | ActiveState::Failed => Before::Start,
    }
}

// ---------------------------------------------------------------------------------------------------
// Answers to a start
// ---------------------------------------------------------------------------------------------------

/// The answer to `StartUnit`, already taken off the bus, so that its meaning can be tested without it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartAnswer {
    /// systemd accepted the job.
    Started,
    /// An error answer with this name.
    Error(String),
    /// No system bus, no answer in time, or an answer of another form.
    NoBus,
}

/// Why a start did not lead to a running service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartFailure {
    /// polkit does not allow the start without a password (no rule, or it does not apply in this session).
    NeedsPassword,
    /// The service is not set up.
    NotInstalled,
    /// The start was accepted, but the service stopped again at once (or systemd refused the job).
    Failed,
    /// No answer of the control socket within [`START_WAIT`].
    NoAnswer,
    /// systemd could not be reached.
    NoSystemBus,
}

/// What is known about starting without a password in this session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Permit {
    /// Not tried yet (the service was running when Celata started).
    #[default]
    Unknown,
    Allowed,
    NeedsPassword,
}

impl Permit {
    /// The permit after the result of a start without a password prompt. Only success and "needs a password"
    /// tell something about polkit; every other reason leaves it as it is.
    #[must_use]
    pub fn after(self, result: &Result<(), StartFailure>) -> Self {
        match result {
            Ok(()) => Self::Allowed,
            Err(StartFailure::NeedsPassword) => Self::NeedsPassword,
            Err(_) => self,
        }
    }
}

/// Result of the start after the button "Start service" (with a password prompt).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interactive {
    Started,
    /// The prompt was cancelled or the password was wrong: nothing changes, no message.
    Dismissed,
    /// The system could not ask for the password (no agent): one message with the command for the terminal.
    NoPrompt,
    Failed(StartFailure),
}

/// Meaning of the answer to a start without a password prompt.
pub fn classify(answer: &StartAnswer) -> Result<(), StartFailure> {
    match answer {
        StartAnswer::Started => Ok(()),
        StartAnswer::Error(name) => Err(failure_of(name, StartFailure::NeedsPassword)),
        StartAnswer::NoBus => Err(StartFailure::NoSystemBus),
    }
}

/// Meaning of the answer to the start with a password prompt.
pub fn classify_interactive(answer: &StartAnswer) -> Interactive {
    match answer {
        StartAnswer::Started => Interactive::Started,
        StartAnswer::Error(name) if name == ERROR_ACCESS_DENIED => Interactive::Dismissed,
        StartAnswer::Error(name) if name == ERROR_INTERACTIVE => Interactive::NoPrompt,
        StartAnswer::Error(name) => Interactive::Failed(failure_of(name, StartFailure::Failed)),
        StartAnswer::NoBus => Interactive::Failed(StartFailure::NoSystemBus),
    }
}

/// Reason for an error name; `denied` is the reason for a refusal by polkit.
fn failure_of(name: &str, denied: StartFailure) -> StartFailure {
    match name {
        ERROR_INTERACTIVE | ERROR_ACCESS_DENIED => denied,
        ERROR_NO_SUCH_UNIT | ERROR_UNIT_MASKED => StartFailure::NotInstalled,
        // For example `TransactionIsDestructive` (a stop is queued) or `JobTypeNotApplicable`.
        _ => StartFailure::Failed,
    }
}

// ---------------------------------------------------------------------------------------------------
// Progress of a start
// ---------------------------------------------------------------------------------------------------

/// How a running start looks right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    Waiting,
    /// The unit is active: its socket now has the rights of the group; errors of the socket count again.
    Ready,
    /// The service stopped again (systemd may restart it by itself; Celata does not try again).
    Failed,
    /// [`START_WAIT`] passed without the unit becoming active.
    TimedOut,
}

/// Decides from the state of the unit and the time since the start.
pub fn progress(unit: &UnitInfo, elapsed: Duration) -> Progress {
    if matches!(unit.active, ActiveState::Active | ActiveState::Reloading) {
        return Progress::Ready;
    }
    let failed = unit.active == ActiveState::Failed
        || unit.sub.starts_with("auto-restart")
        || (unit.active == ActiveState::Inactive && !unit.job);
    if failed {
        Progress::Failed
    } else if elapsed >= START_WAIT {
        Progress::TimedOut
    } else {
        Progress::Waiting
    }
}

/// Progress from one reading during a start. A failed reading only waits. After [`START_WAIT`] the start ends as
/// timed out even with an active unit: then the control socket did not answer in time.
pub fn poll(unit: Result<&UnitInfo, &BusError>, elapsed: Duration) -> Progress {
    let progress = match unit {
        Ok(unit) => progress(unit, elapsed),
        Err(_) if elapsed >= START_WAIT => Progress::TimedOut,
        Err(_) => Progress::Waiting,
    };
    match progress {
        Progress::Ready if elapsed >= START_WAIT => Progress::TimedOut,
        other => other,
    }
}

// ---------------------------------------------------------------------------------------------------
// Command for "Fix permanently"
// ---------------------------------------------------------------------------------------------------

/// The command that sets up the rule by hand: the running program prints it, `sudo tee` writes it. The path is
/// put in single quotes, a quote in it written as `'\''`; without a known path the plain name `celata` stands.
pub fn rule_command(exe: Option<&Path>) -> String {
    let program = match exe.and_then(Path::to_str) {
        Some(path) => format!("'{}'", path.replace('\'', r"'\''")),
        None => "celata".to_owned(),
    };
    format!("{program} --polkit-rule | sudo tee {RULE_PATH} >/dev/null")
}

// ---------------------------------------------------------------------------------------------------
// System bus: the only place in Celata that talks to systemd
// ---------------------------------------------------------------------------------------------------

const SYSTEMD: &str = "org.freedesktop.systemd1";
const MANAGER_PATH: &str = "/org/freedesktop/systemd1";
const MANAGER: &str = "org.freedesktop.systemd1.Manager";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
const UNIT_INTERFACE: &str = "org.freedesktop.systemd1.Unit";
const SERVICE_INTERFACE: &str = "org.freedesktop.systemd1.Service";

/// A connection to the system bus of its own, with a time limit for every call.
fn system(timeout: Duration) -> Result<Connection, BusError> {
    zbus::blocking::connection::Builder::system()
        .map_err(|_| BusError::NoBus)?
        .method_timeout(timeout)
        .build()
        .map_err(|_| BusError::NoBus)
}

fn text_property(properties: &HashMap<String, OwnedValue>, name: &str) -> Option<String> {
    properties
        .get(name)
        .and_then(|value| value.downcast_ref::<&str>().ok())
        .map(str::to_owned)
}

/// Whether the property `Job` (`(u, o)`) names a job; `(0, "/")` means none.
fn has_job(properties: &HashMap<String, OwnedValue>) -> Option<bool> {
    let job = properties.get("Job")?.downcast_ref::<Structure>().ok()?;
    let id = job.fields().first()?.downcast_ref::<u32>().ok()?;
    Some(id != 0)
}

/// One call to systemd on `connection`.
fn call<B>(connection: &Connection, path: &str, interface: &str, method: &str, body: &B) -> Result<Message, BusError>
where
    B: serde::Serialize + DynamicType,
{
    connection
        .call_method(Some(SYSTEMD), path, Some(interface), method, body)
        .map_err(|_| BusError::NoAnswer)
}

/// Reads the state of the unit `name`. Needs no rights; `LoadUnit` also answers for a unit that does not
/// exist (`LoadState` `not-found`).
pub fn read_unit(name: &str) -> Result<UnitInfo, BusError> {
    let connection = system(BUS_TIMEOUT)?;
    let path: OwnedObjectPath = call(&connection, MANAGER_PATH, MANAGER, "LoadUnit", &(name,))?
        .body()
        .deserialize()
        .map_err(|_| BusError::Unexpected)?;
    let properties: HashMap<String, OwnedValue> =
        call(&connection, path.as_str(), PROPERTIES, "GetAll", &(UNIT_INTERFACE,))?
            .body()
            .deserialize()
            .map_err(|_| BusError::Unexpected)?;
    let load = text_property(&properties, "LoadState").ok_or(BusError::Unexpected)?;
    let active = text_property(&properties, "ActiveState").ok_or(BusError::Unexpected)?;
    // The service interface is missing for a unit that does not exist; its result then stays unknown.
    let result = call(
        &connection,
        path.as_str(),
        PROPERTIES,
        "Get",
        &(SERVICE_INTERFACE, "Result"),
    )
    .ok()
    .and_then(|reply| reply.body().deserialize::<OwnedValue>().ok())
    .and_then(|value| value.downcast_ref::<&str>().ok().map(str::to_owned))
    .unwrap_or_default();
    Ok(UnitInfo {
        load: LoadState::parse(&load),
        active: ActiveState::parse(&active),
        sub: text_property(&properties, "SubState").unwrap_or_default(),
        result,
        job: has_job(&properties).ok_or(BusError::Unexpected)?,
    })
}

/// Translates the answer of `StartUnit`.
fn start_answer(result: zbus::Result<()>) -> StartAnswer {
    match result {
        Ok(()) => StartAnswer::Started,
        Err(zbus::Error::MethodError(name, _, _)) => StartAnswer::Error(name.as_str().to_owned()),
        Err(_) => StartAnswer::NoBus,
    }
}

/// Starts the unit `name` without a password prompt: polkit answers at once, and systemd only accepts the job if
/// the rule allows it. The job mode `fail` refuses the start while a conflicting job (for example a stop the user
/// started as the emergency exit) is queued, instead of replacing it as `replace` would.
pub fn start_unit(name: &str) -> StartAnswer {
    let Ok(connection) = system(BUS_TIMEOUT) else {
        return StartAnswer::NoBus;
    };
    start_answer(
        connection
            .call_method(Some(SYSTEMD), MANAGER_PATH, Some(MANAGER), "StartUnit", &(name, "fail"))
            .map(|_| ()),
    )
}

/// Starts the unit `name` and allows polkit to ask for the password through the agent of the desktop. Only the
/// button "Start service" calls this; Celata never sees the password.
pub fn start_unit_interactive(name: &str) -> StartAnswer {
    let Ok(connection) = system(PROMPT_TIMEOUT) else {
        return StartAnswer::NoBus;
    };
    let proxy = zbus::blocking::proxy::Builder::<zbus::blocking::Proxy>::new(&connection)
        .destination(SYSTEMD)
        .and_then(|builder| builder.path(MANAGER_PATH))
        .and_then(|builder| builder.interface(MANAGER))
        .map(|builder| builder.cache_properties(zbus::proxy::CacheProperties::No))
        .and_then(zbus::blocking::proxy::Builder::build);
    let Ok(proxy) = proxy else {
        return StartAnswer::NoBus;
    };
    start_answer(
        proxy
            .call_with_flags::<_, _, OwnedObjectPath>(
                "StartUnit",
                zbus::proxy::MethodFlags::AllowInteractiveAuth.into(),
                &(name, "fail"),
            )
            .map(|_| ()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const GROUP_FILE: &str = "root:x:0:\nwheel:x:10:martin\ncelata2:x:977:\ncelata:x:978:martin\nlp:x:7:";

    fn status(groups: &str) -> String {
        format!("Name:\tcelata\nUid:\t1000\t1000\t1000\t1000\nGroups:{groups}\nVmPeak:\t1 kB\n")
    }

    #[test]
    fn gruppe_wird_am_eigenen_prozess_geprueft() {
        assert_eq!(
            group_check(&status("\t10 978 1000 "), GROUP_FILE, GROUP),
            GroupCheck::Member
        );
        assert_eq!(
            group_check(&status("\t10\t978\t1000"), GROUP_FILE, GROUP),
            GroupCheck::Member
        );
        // Known number, missing from the groups of the process.
        assert_eq!(
            group_check(&status("\t10 1000"), GROUP_FILE, GROUP),
            GroupCheck::NotMember
        );
        assert_eq!(group_check(&status(""), GROUP_FILE, GROUP), GroupCheck::NotMember);
        // A name that only contains "celata" does not count.
        assert_eq!(group_check(&status("\t977"), GROUP_FILE, GROUP), GroupCheck::NotMember);
        // Group as last line without a line end.
        assert_eq!(group_check(&status("\t7"), GROUP_FILE, "lp"), GroupCheck::Member);
        // The group is unknown, the line is missing, or the files could not be read.
        assert_eq!(group_check(&status("\t978"), "root:x:0:\n", GROUP), GroupCheck::Unknown);
        assert_eq!(group_check("Name:\tcelata\n", GROUP_FILE, GROUP), GroupCheck::Unknown);
        assert_eq!(group_check("", "", GROUP), GroupCheck::Unknown);
        assert_eq!(group_check(&status("\t978"), "", GROUP), GroupCheck::Unknown);
        assert_eq!(group_check("", GROUP_FILE, GROUP), GroupCheck::Unknown);
        assert_eq!(
            group_check(&status("\t978"), "celata:x:abc:\n", GROUP),
            GroupCheck::Unknown
        );
    }

    fn unit(load: LoadState, active: ActiveState, job: bool) -> UnitInfo {
        UnitInfo {
            load,
            active,
            sub: String::new(),
            result: String::new(),
            job,
        }
    }

    #[test]
    fn unbekannte_zustaende_werden_other() {
        assert_eq!(LoadState::parse("loaded"), LoadState::Loaded);
        assert_eq!(LoadState::parse("not-found"), LoadState::NotFound);
        assert_eq!(LoadState::parse("masked"), LoadState::Masked);
        assert_eq!(LoadState::parse("bad-setting"), LoadState::Other);
        assert_eq!(LoadState::parse(""), LoadState::Other);
        for (text, state) in [
            ("active", ActiveState::Active),
            ("activating", ActiveState::Activating),
            ("deactivating", ActiveState::Deactivating),
            ("reloading", ActiveState::Reloading),
            ("inactive", ActiveState::Inactive),
            ("failed", ActiveState::Failed),
            ("maintenance", ActiveState::Other),
            ("ACTIVE", ActiveState::Other),
        ] {
            assert_eq!(ActiveState::parse(text), state, "{text}");
        }
    }

    #[test]
    fn entscheidung_vor_dem_start_in_fester_reihenfolge() {
        use ActiveState::*;
        let loads = [
            LoadState::Loaded,
            LoadState::NotFound,
            LoadState::Masked,
            LoadState::Other,
        ];
        let actives = [Active, Activating, Deactivating, Reloading, Inactive, Failed, Other];
        let groups = [GroupCheck::Member, GroupCheck::NotMember, GroupCheck::Unknown];
        for group in groups {
            assert_eq!(before_start(Err(&BusError::NoBus), group), Before::NoSystemBus);
            assert_eq!(before_start(Err(&BusError::NoAnswer), group), Before::NoSystemBus);
            assert_eq!(before_start(Err(&BusError::Unexpected), group), Before::NoSystemBus);
            for load in loads {
                for active in actives {
                    for job in [false, true] {
                        let info = unit(load, active, job);
                        let expected = if matches!(load, LoadState::NotFound | LoadState::Masked) {
                            Before::NotInstalled
                        } else if group == GroupCheck::NotMember {
                            Before::NoPermission
                        } else if matches!(active, Active | Reloading) {
                            Before::Running
                        } else if job || matches!(active, Activating | Deactivating | Other) {
                            Before::Wait
                        } else {
                            Before::Start
                        };
                        assert_eq!(
                            before_start(Ok(&info), group),
                            expected,
                            "{load:?} {active:?} job={job} {group:?}"
                        );
                    }
                }
            }
        }
        // The usual cases spelled out.
        let start = unit(LoadState::Loaded, Inactive, false);
        assert_eq!(before_start(Ok(&start), GroupCheck::Member), Before::Start);
        assert_eq!(before_start(Ok(&start), GroupCheck::Unknown), Before::Start);
        let failed = unit(LoadState::Loaded, Failed, false);
        assert_eq!(before_start(Ok(&failed), GroupCheck::Member), Before::Start);
        // A running stop is waited for, not replaced.
        let stopping = unit(LoadState::Loaded, Deactivating, true);
        assert_eq!(before_start(Ok(&stopping), GroupCheck::Member), Before::Wait);
        let boot = unit(LoadState::Loaded, Inactive, true);
        assert_eq!(before_start(Ok(&boot), GroupCheck::Member), Before::Wait);
    }

    fn error(name: &str) -> StartAnswer {
        StartAnswer::Error(name.to_owned())
    }

    #[test]
    fn antworten_ohne_passwortabfrage() {
        assert_eq!(classify(&StartAnswer::Started), Ok(()));
        assert_eq!(classify(&error(ERROR_INTERACTIVE)), Err(StartFailure::NeedsPassword));
        assert_eq!(classify(&error(ERROR_ACCESS_DENIED)), Err(StartFailure::NeedsPassword));
        assert_eq!(classify(&error(ERROR_NO_SUCH_UNIT)), Err(StartFailure::NotInstalled));
        assert_eq!(classify(&error(ERROR_UNIT_MASKED)), Err(StartFailure::NotInstalled));
        for name in [
            "org.freedesktop.systemd1.TransactionIsDestructive",
            "org.freedesktop.systemd1.JobTypeNotApplicable",
            "org.example.Unknown",
            "",
        ] {
            assert_eq!(classify(&error(name)), Err(StartFailure::Failed), "{name}");
        }
        assert_eq!(classify(&StartAnswer::NoBus), Err(StartFailure::NoSystemBus));
    }

    #[test]
    fn antworten_nach_dem_knopf() {
        assert_eq!(classify_interactive(&StartAnswer::Started), Interactive::Started);
        assert_eq!(
            classify_interactive(&error(ERROR_ACCESS_DENIED)),
            Interactive::Dismissed
        );
        assert_eq!(classify_interactive(&error(ERROR_INTERACTIVE)), Interactive::NoPrompt);
        assert_eq!(
            classify_interactive(&error(ERROR_NO_SUCH_UNIT)),
            Interactive::Failed(StartFailure::NotInstalled)
        );
        assert_eq!(
            classify_interactive(&error(ERROR_UNIT_MASKED)),
            Interactive::Failed(StartFailure::NotInstalled)
        );
        assert_eq!(
            classify_interactive(&error("org.freedesktop.systemd1.TransactionIsDestructive")),
            Interactive::Failed(StartFailure::Failed)
        );
        assert_eq!(
            classify_interactive(&error("org.example.Unknown")),
            Interactive::Failed(StartFailure::Failed)
        );
        assert_eq!(
            classify_interactive(&StartAnswer::NoBus),
            Interactive::Failed(StartFailure::NoSystemBus)
        );
    }

    #[test]
    fn erlaubnis_nur_aus_erfolg_oder_passwort() {
        for permit in [Permit::Unknown, Permit::Allowed, Permit::NeedsPassword] {
            assert_eq!(permit.after(&Ok(())), Permit::Allowed);
            assert_eq!(permit.after(&Err(StartFailure::NeedsPassword)), Permit::NeedsPassword);
            for other in [
                StartFailure::NotInstalled,
                StartFailure::Failed,
                StartFailure::NoAnswer,
                StartFailure::NoSystemBus,
            ] {
                assert_eq!(permit.after(&Err(other)), permit, "{permit:?} {other:?}");
            }
        }
    }

    #[test]
    fn fortschritt_eines_starts() {
        let second = Duration::from_secs(1);
        let at = |active, sub: &str, job| UnitInfo {
            sub: sub.to_owned(),
            ..unit(LoadState::Loaded, active, job)
        };
        assert_eq!(
            progress(&at(ActiveState::Active, "running", false), second),
            Progress::Ready
        );
        assert_eq!(
            progress(&at(ActiveState::Activating, "start", true), second),
            Progress::Waiting
        );
        assert_eq!(
            progress(&at(ActiveState::Activating, "auto-restart", false), second),
            Progress::Failed
        );
        assert_eq!(
            progress(&at(ActiveState::Activating, "auto-restart-queued", true), second),
            Progress::Failed
        );
        assert_eq!(
            progress(&at(ActiveState::Failed, "failed", false), second),
            Progress::Failed
        );
        assert_eq!(
            progress(&at(ActiveState::Inactive, "dead", false), second),
            Progress::Failed
        );
        assert_eq!(
            progress(&at(ActiveState::Inactive, "dead", true), second),
            Progress::Waiting
        );
        let starting = at(ActiveState::Activating, "start", true);
        assert_eq!(progress(&starting, Duration::from_secs(89)), Progress::Waiting);
        assert_eq!(progress(&starting, START_WAIT), Progress::TimedOut);
        assert_eq!(
            progress(&at(ActiveState::Active, "running", false), START_WAIT),
            Progress::Ready
        );
        assert_eq!(START_WAIT, Duration::from_secs(90));
    }

    #[test]
    fn abfrage_waehrend_eines_starts() {
        let active = UnitInfo {
            sub: "running".to_owned(),
            ..unit(LoadState::Loaded, ActiveState::Active, false)
        };
        let starting = unit(LoadState::Loaded, ActiveState::Activating, true);
        let second = Duration::from_secs(1);
        assert_eq!(poll(Ok(&active), second), Progress::Ready);
        assert_eq!(poll(Ok(&starting), second), Progress::Waiting);
        // A failed reading waits, until the time is over.
        assert_eq!(poll(Err(&BusError::NoAnswer), second), Progress::Waiting);
        assert_eq!(poll(Err(&BusError::NoAnswer), START_WAIT), Progress::TimedOut);
        // Active, but the socket did not answer within the time.
        assert_eq!(poll(Ok(&active), START_WAIT), Progress::TimedOut);
        assert_eq!(poll(Ok(&starting), START_WAIT), Progress::TimedOut);
        let failed = unit(LoadState::Loaded, ActiveState::Failed, false);
        assert_eq!(poll(Ok(&failed), START_WAIT), Progress::Failed);
    }

    #[test]
    fn befehl_fuer_dauerhaft_beheben() {
        assert_eq!(
            rule_command(Some(Path::new("/usr/bin/celata"))),
            "'/usr/bin/celata' --polkit-rule | sudo tee /etc/polkit-1/rules.d/49-celata.rules >/dev/null"
        );
        assert_eq!(
            rule_command(Some(Path::new("/home/a b/celata"))),
            "'/home/a b/celata' --polkit-rule | sudo tee /etc/polkit-1/rules.d/49-celata.rules >/dev/null"
        );
        assert_eq!(
            rule_command(Some(Path::new("/opt/it's/celata"))),
            r"'/opt/it'\''s/celata' --polkit-rule | sudo tee /etc/polkit-1/rules.d/49-celata.rules >/dev/null"
        );
        assert_eq!(
            rule_command(None),
            "celata --polkit-rule | sudo tee /etc/polkit-1/rules.d/49-celata.rules >/dev/null"
        );
        assert_eq!(RULE, include_str!("../packaging/polkit/49-celata.rules"));
        assert!(RULE.ends_with("});\n"));
    }
}
