// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! Settings of the hide.me service that Celata manages.
//!
//! The service forgets the values it was given on every restart and must not be read out
//! (`GET /configuration` returns credentials). Celata therefore stores the settings itself and
//! sends them completely before every connection. The defaults match `NewConfiguration` in
//! `configuration.go` of hide.me v0.9.14.
//!
//! [`load`] does not hand out settings that demonstrably have no effect with the approved version of
//! hide.me ([`FILTERS_WORK`]).

use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Read};
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::dnsleak::Assessment;
use crate::files;
use crate::i18n::{Key, Language, Phrase};

/// The 18 DNS-over-HTTPS servers from `configuration.go` (v0.9.14), taken over literally.
pub const DEFAULT_DOH_SERVERS: [&str; 18] = [
    "sdns://AgYAAAAAAAAADjE0OS4xMTIuMTEyLjEwILAZIHRLu3bJqwU-AeB7fgUORz0g95976kNfr-Q8nSQvE2RuczEwLnF1YWQ5Lm5ldDo0NDMKL2Rucy1xdWVyeQ",
    "sdns://AgYAAAAAAAAACDkuOS45LjEwILAZIHRLu3bJqwU-AeB7fgUORz0g95976kNfr-Q8nSQvE2RuczEwLnF1YWQ5Lm5ldDo0NDMKL2Rucy1xdWVyeQ",
    "sdns://AgYAAAAAAAAADjE0OS4xMTIuMTEyLjEwILAZIHRLu3bJqwU-AeB7fgUORz0g95976kNfr-Q8nSQvFGRuczEwLnF1YWQ5Lm5ldDo1MDUzCi9kbnMtcXVlcnk",
    "sdns://AgYAAAAAAAAACDkuOS45LjEwILAZIHRLu3bJqwU-AeB7fgUORz0g95976kNfr-Q8nSQvFGRuczEwLnF1YWQ5Lm5ldDo1MDUzCi9kbnMtcXVlcnk",
    "sdns://AgMAAAAAAAAADjE0OS4xMTIuMTEyLjExILAZIHRLu3bJqwU-AeB7fgUORz0g95976kNfr-Q8nSQvE2RuczExLnF1YWQ5Lm5ldDo0NDMKL2Rucy1xdWVyeQ",
    "sdns://AgMAAAAAAAAACDkuOS45LjExILAZIHRLu3bJqwU-AeB7fgUORz0g95976kNfr-Q8nSQvE2RuczExLnF1YWQ5Lm5ldDo0NDMKL2Rucy1xdWVyeQ",
    "sdns://AgMAAAAAAAAADjE0OS4xMTIuMTEyLjExILAZIHRLu3bJqwU-AeB7fgUORz0g95976kNfr-Q8nSQvFGRuczExLnF1YWQ5Lm5ldDo1MDUzCi9kbnMtcXVlcnk",
    "sdns://AgMAAAAAAAAACDkuOS45LjExILAZIHRLu3bJqwU-AeB7fgUORz0g95976kNfr-Q8nSQvFGRuczExLnF1YWQ5Lm5ldDo1MDUzCi9kbnMtcXVlcnk",
    "sdns://AgYAAAAAAAAADjE0OS4xMTIuMTEyLjEyILAZIHRLu3bJqwU-AeB7fgUORz0g95976kNfr-Q8nSQvE2RuczEyLnF1YWQ5Lm5ldDo0NDMKL2Rucy1xdWVyeQ",
    "sdns://AgYAAAAAAAAACDkuOS45LjEyILAZIHRLu3bJqwU-AeB7fgUORz0g95976kNfr-Q8nSQvE2RuczEyLnF1YWQ5Lm5ldDo0NDMKL2Rucy1xdWVyeQ",
    "sdns://AgYAAAAAAAAADjE0OS4xMTIuMTEyLjEyILAZIHRLu3bJqwU-AeB7fgUORz0g95976kNfr-Q8nSQvFGRuczEyLnF1YWQ5Lm5ldDo1MDUzCi9kbnMtcXVlcnk",
    "sdns://AgYAAAAAAAAACDkuOS45LjEyILAZIHRLu3bJqwU-AeB7fgUORz0g95976kNfr-Q8nSQvFGRuczEyLnF1YWQ5Lm5ldDo1MDUzCi9kbnMtcXVlcnk",
    "sdns://AgMAAAAAAAAADTE0OS4xMTIuMTEyLjkgsBkgdEu7dsmrBT4B4Ht-BQ5HPSD3n3vqQ1-v5DydJC8SZG5zOS5xdWFkOS5uZXQ6NDQzCi9kbnMtcXVlcnk",
    "sdns://AgMAAAAAAAAABzkuOS45LjkgsBkgdEu7dsmrBT4B4Ht-BQ5HPSD3n3vqQ1-v5DydJC8SZG5zOS5xdWFkOS5uZXQ6NDQzCi9kbnMtcXVlcnk",
    "sdns://AgMAAAAAAAAADTE0OS4xMTIuMTEyLjkgsBkgdEu7dsmrBT4B4Ht-BQ5HPSD3n3vqQ1-v5DydJC8TZG5zOS5xdWFkOS5uZXQ6NTA1MwovZG5zLXF1ZXJ5",
    "sdns://AgMAAAAAAAAABzkuOS45LjkgsBkgdEu7dsmrBT4B4Ht-BQ5HPSD3n3vqQ1-v5DydJC8TZG5zOS5xdWFkOS5uZXQ6NTA1MwovZG5zLXF1ZXJ5",
    "sdns://AgMAAAAAAAAADzE0OS4xMTIuMTEyLjExMiCwGSB0S7t2yasFPgHge34FDkc9IPefe-pDX6_kPJ0kLxFkbnMucXVhZDkubmV0OjQ0MwovZG5zLXF1ZXJ5",
    "sdns://AgMAAAAAAAAADzE0OS4xMTIuMTEyLjExMiCwGSB0S7t2yasFPgHge34FDkc9IPefe-pDX6_kPJ0kLxJkbnMucXVhZDkubmV0OjUwNTMKL2Rucy1xdWVyeQ",
];

/// Fallback DNS servers from `configuration.go` (v0.9.14).
pub const DEFAULT_PLAIN_DNS: [&str; 2] = ["209.250.251.37:53", "217.182.206.81:53"];

/// Directory that always holds the backup of `resolv.conf`.
pub const BACKUP_DIR: &str = "/run/hide.me";

/// Risk levels and illegal content that `Filter.Check` of the service accepts.
pub const RISK_LEVELS: [&str; 3] = ["possible", "medium", "high"];
pub const ILLEGAL_KINDS: [&str; 4] = ["content", "warez", "spyware", "copyright"];
/// Parental control levels that `Filter.Check` of the service accepts.
pub const PG_LEVELS: [u8; 4] = [0, 12, 18, 21];

/// IPv4 only, IPv6 only or both.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub enum IpVersion {
    #[default]
    Both,
    V4Only,
    V6Only,
}

impl IpVersion {
    pub fn key(self) -> &'static str {
        match self {
            Self::Both => "both",
            Self::V4Only => "v4",
            Self::V6Only => "v6",
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        [Self::Both, Self::V4Only, Self::V6Only]
            .into_iter()
            .find(|version| version.key() == key)
    }
}

/// `SmartGuard` filters of the hide.me server (`rest/filter.go`).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
#[serde(default)]
pub struct Filter {
    pub ads: bool,
    pub trackers: bool,
    pub malware: bool,
    pub malicious: bool,
    pub pg: u8,
    pub safe_search: bool,
    pub risk: BTreeSet<String>,
    pub illegal: BTreeSet<String>,
    pub categories: BTreeSet<String>,
    pub whitelist: Vec<String>,
    pub blacklist: Vec<String>,
    pub force_dns: bool,
}

impl Filter {
    /// Is no filter selected? Like `Filter.Empty` in the service.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Level of "Block risky sites": 0 = off, 1 = high risk, 2 = medium and up, 3 = possible and up.
/// Other combinations (for example from an older file) show the closest level.
pub fn risk_level(risk: &BTreeSet<String>) -> u8 {
    if risk.contains("possible") {
        3
    } else if risk.contains("medium") {
        2
    } else {
        u8::from(risk.contains("high"))
    }
}

/// Risk levels for a level from [`risk_level`].
pub fn risk_for_level(level: u8) -> Option<BTreeSet<String>> {
    let count = match level {
        0..=3 => usize::from(level),
        _ => return None,
    };
    // From "high" upwards: `RISK_LEVELS` is sorted by increasing strictness ("possible" first).
    Some(
        RISK_LEVELS
            .iter()
            .rev()
            .take(count)
            .map(|level| (*level).to_owned())
            .collect(),
    )
}

/// Do the 12 filter settings ([`Filter`]) have an effect with the approved version of hide.me? The only
/// place that decides this.
///
/// Finding, confirmed against the real service: hide.me v0.9.14 sends the filter request without the
/// access token (`json:"-"` in `rest/filter.go`, commit `ffb1c0f`), and the hide.me server rejects it with
/// HTTP 401. The whole object `Rest.Filter` is affected, including `forceDns`. Fixed in `master` after
/// v0.9.14.
///
/// While `false`: Celata does not show the section "Filters" or "All DNS requests via hide.me", does not
/// hand out a stored selection from [`load`] and sends the service only defaults for them. Set it to
/// `true` only after the filters have been tested again successfully with a new version of hide.me.
pub const FILTERS_WORK: bool = false;

/// Splits off the hidden settings: the result is the settings Celata uses and the selection that is
/// held back. With working filters everything stays as it is.
fn split(mut settings: Settings, filters_work: bool) -> (Settings, Filter) {
    if filters_work {
        (settings, Filter::default())
    } else {
        let kept = std::mem::take(&mut settings.filter);
        (settings, kept)
    }
}

/// Counterpart of [`split`] for saving: puts the selection that was held back in again.
fn merge(settings: &Settings, kept: &Filter, filters_work: bool) -> Settings {
    if filters_work {
        settings.clone()
    } else {
        Settings {
            filter: kept.clone(),
            ..settings.clone()
        }
    }
}

/// `settings` without hidden values. For comparisons with what an earlier version of Celata sent to
/// the service.
pub fn visible(settings: &Settings) -> Settings {
    split(settings.clone(), FILTERS_WORK).0
}

/// Result of "Reset all settings to defaults", including hidden settings: `None` if there is nothing
/// to reset (the user interface then does not ask), otherwise the new settings and the new, empty
/// selection that is held back.
pub fn reset_all(current: &Settings, kept: &Filter) -> Option<(Settings, Filter)> {
    if current.is_default() && kept.is_empty() {
        None
    } else {
        Some((Settings::default(), Filter::default()))
    }
}

/// The set of settings that Celata manages. 29 fields: 12 in the filter, 17 here.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(default)]
pub struct Settings {
    pub filter: Filter,
    pub use_doh: bool,
    pub plain_dns: Vec<String>,
    pub split_tunnel: Vec<String>,
    pub port_forward: bool,
    pub kill_switch: bool,
    pub ip_version: IpVersion,
    pub port: u16,
    pub dpd_secs: u32,
    pub rest_timeout_secs: u32,
    pub reconnect_wait_secs: u32,
    pub interface: String,
    pub listen_port: u16,
    pub mark: u32,
    pub routing_table: u32,
    pub rule_priority: u32,
    pub resolv_backup: String,
    pub doh_servers: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            filter: Filter::default(),
            use_doh: true,
            plain_dns: DEFAULT_PLAIN_DNS.iter().map(|server| (*server).to_owned()).collect(),
            split_tunnel: Vec::new(),
            port_forward: false,
            kill_switch: true,
            ip_version: IpVersion::Both,
            port: 432,
            dpd_secs: 60,
            rest_timeout_secs: 90,
            reconnect_wait_secs: 30,
            interface: "vpn".to_owned(),
            listen_port: 0,
            mark: 0,
            routing_table: 55555,
            rule_priority: 10,
            resolv_backup: String::new(),
            doh_servers: DEFAULT_DOH_SERVERS.iter().map(|stamp| (*stamp).to_owned()).collect(),
        }
    }
}

impl Settings {
    /// Are all values at their defaults?
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// A single field of the set of settings. `key` is the name the user interface uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Field {
    Ads,
    Trackers,
    Malware,
    Malicious,
    Pg,
    SafeSearch,
    Risk,
    Illegal,
    Categories,
    Whitelist,
    Blacklist,
    ForceDns,
    UseDoh,
    PlainDns,
    SplitTunnel,
    PortForward,
    KillSwitch,
    IpVersion,
    Port,
    DpdSecs,
    RestTimeoutSecs,
    ReconnectWaitSecs,
    Interface,
    ListenPort,
    Mark,
    RoutingTable,
    RulePriority,
    ResolvBackup,
    DohServers,
}

impl Field {
    pub const ALL: [Field; 29] = [
        Self::Ads,
        Self::Trackers,
        Self::Malware,
        Self::Malicious,
        Self::Pg,
        Self::SafeSearch,
        Self::Risk,
        Self::Illegal,
        Self::Categories,
        Self::Whitelist,
        Self::Blacklist,
        Self::ForceDns,
        Self::UseDoh,
        Self::PlainDns,
        Self::SplitTunnel,
        Self::PortForward,
        Self::KillSwitch,
        Self::IpVersion,
        Self::Port,
        Self::DpdSecs,
        Self::RestTimeoutSecs,
        Self::ReconnectWaitSecs,
        Self::Interface,
        Self::ListenPort,
        Self::Mark,
        Self::RoutingTable,
        Self::RulePriority,
        Self::ResolvBackup,
        Self::DohServers,
    ];

    /// Name of the field in the user interface.
    pub fn key(self) -> &'static str {
        match self {
            Self::Ads => "ads",
            Self::Trackers => "trackers",
            Self::Malware => "malware",
            Self::Malicious => "malicious",
            Self::Pg => "pg",
            Self::SafeSearch => "safe_search",
            Self::Risk => "risk",
            Self::Illegal => "illegal",
            Self::Categories => "category",
            Self::Whitelist => "whitelist",
            Self::Blacklist => "blacklist",
            Self::ForceDns => "force_dns",
            Self::UseDoh => "use_doh",
            Self::PlainDns => "plain_dns",
            Self::SplitTunnel => "split_tunnel",
            Self::PortForward => "port_forward",
            Self::KillSwitch => "kill_switch",
            Self::IpVersion => "ip_version",
            Self::Port => "port",
            Self::DpdSecs => "dpd_secs",
            Self::RestTimeoutSecs => "rest_timeout_secs",
            Self::ReconnectWaitSecs => "reconnect_wait_secs",
            Self::Interface => "interface",
            Self::ListenPort => "listen_port",
            Self::Mark => "mark",
            Self::RoutingTable => "routing_table",
            Self::RulePriority => "rule_priority",
            Self::ResolvBackup => "resolv_backup",
            Self::DohServers => "doh_servers",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|field| field.key() == key)
    }

    /// Does the field belong to the section "Advanced"?
    pub fn is_advanced(self) -> bool {
        matches!(
            self,
            Self::IpVersion
                | Self::Port
                | Self::DpdSecs
                | Self::RestTimeoutSecs
                | Self::ReconnectWaitSecs
                | Self::Interface
                | Self::ListenPort
                | Self::Mark
                | Self::RoutingTable
                | Self::RulePriority
                | Self::ResolvBackup
                | Self::DohServers
        )
    }

    /// Does the field differ between `a` and `b`?
    fn differs(self, a: &Settings, b: &Settings) -> bool {
        let (fa, fb) = (&a.filter, &b.filter);
        match self {
            Self::Ads => fa.ads != fb.ads,
            Self::Trackers => fa.trackers != fb.trackers,
            Self::Malware => fa.malware != fb.malware,
            Self::Malicious => fa.malicious != fb.malicious,
            Self::Pg => fa.pg != fb.pg,
            Self::SafeSearch => fa.safe_search != fb.safe_search,
            Self::Risk => fa.risk != fb.risk,
            Self::Illegal => fa.illegal != fb.illegal,
            Self::Categories => fa.categories != fb.categories,
            Self::Whitelist => fa.whitelist != fb.whitelist,
            Self::Blacklist => fa.blacklist != fb.blacklist,
            Self::ForceDns => fa.force_dns != fb.force_dns,
            Self::UseDoh => a.use_doh != b.use_doh,
            Self::PlainDns => a.plain_dns != b.plain_dns,
            Self::SplitTunnel => a.split_tunnel != b.split_tunnel,
            Self::PortForward => a.port_forward != b.port_forward,
            Self::KillSwitch => a.kill_switch != b.kill_switch,
            Self::IpVersion => a.ip_version != b.ip_version,
            Self::Port => a.port != b.port,
            Self::DpdSecs => a.dpd_secs != b.dpd_secs,
            Self::RestTimeoutSecs => a.rest_timeout_secs != b.rest_timeout_secs,
            Self::ReconnectWaitSecs => a.reconnect_wait_secs != b.reconnect_wait_secs,
            Self::Interface => a.interface != b.interface,
            Self::ListenPort => a.listen_port != b.listen_port,
            Self::Mark => a.mark != b.mark,
            Self::RoutingTable => a.routing_table != b.routing_table,
            Self::RulePriority => a.rule_priority != b.rule_priority,
            Self::ResolvBackup => a.resolv_backup != b.resolv_backup,
            Self::DohServers => a.doh_servers != b.doh_servers,
        }
    }
}

/// Fields that differ between `old` and `new`.
pub fn changed_fields(old: &Settings, new: &Settings) -> Vec<Field> {
    Field::ALL.into_iter().filter(|field| field.differs(old, new)).collect()
}

/// How a change is applied while a connection exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    /// "Reconnect": `/disconnect`, the block stays in place.
    Reconnect,
    /// "Rebuild": `/destroy`, the internet is unprotected for a moment.
    Rebuild,
}

/// Class of a field. IP version, DNS-over-HTTPS servers and the request timeout are in "Rebuild" out
/// of caution, until a test against the real service shows otherwise.
pub fn class_of(field: Field) -> Class {
    match field {
        Field::KillSwitch
        | Field::IpVersion
        | Field::RestTimeoutSecs
        | Field::Interface
        | Field::ListenPort
        | Field::Mark
        | Field::RoutingTable
        | Field::RulePriority
        | Field::DohServers => Class::Rebuild,
        _ => Class::Reconnect,
    }
}

/// Highest class of all changed fields (`Rebuild` > `Reconnect`), `None` without a change.
pub fn changed_class(old: &Settings, new: &Settings) -> Option<Class> {
    changed_fields(old, new).into_iter().map(class_of).max()
}

// ---------------------------------------------------------------------------------------------------
// Validation rules
// ---------------------------------------------------------------------------------------------------

/// Error at a field, with text to show below the field (translated only when it is shown).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldError {
    pub field: Field,
    pub message: Phrase,
}

impl FieldError {
    fn new(field: Field, message: impl Into<Phrase>) -> Self {
        Self {
            field,
            message: message.into(),
        }
    }
}

/// Environment that is checked against. Injected for tests.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Environment {
    /// Existing network interfaces.
    pub interfaces: Vec<String>,
}

impl Environment {
    /// Reads the existing network interfaces from `/sys/class/net`.
    pub fn from_system() -> Self {
        let interfaces = fs::read_dir("/sys/class/net")
            .map(|entries| {
                entries
                    .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
                    .collect()
            })
            .unwrap_or_default();
        Self { interfaces }
    }

    /// The same environment without `name`: the name set so far may still be used.
    fn without(&self, name: &str) -> Self {
        Self {
            interfaces: self.interfaces.iter().filter(|other| *other != name).cloned().collect(),
        }
    }
}

// Texts of the field errors: `src/i18n`, keys `Field…`.
const NAME_ERROR: Key = Key::FieldNameError;
const NETWORK_ERROR: Key = Key::FieldNetworkError;
const DNS_ERROR: Key = Key::FieldDnsError;
const STAMP_ERROR: Key = Key::FieldStampError;
const BACKUP_ERROR: Key = Key::FieldBackupError;
const INTERFACE_ERROR: Key = Key::FieldInterfaceError;
const INTERFACE_TAKEN: Key = Key::FieldInterfaceTaken;
const TABLE_RESERVED: Key = Key::FieldTableReserved;

/// Name for the exception lists: 1–253 characters from `A–Z a–z 0–9 _ . - *` (like `nameRegexp` in the service).
pub fn is_valid_filter_name(name: &str) -> bool {
    (1..=253).contains(&name.len())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-' | b'*'))
}

/// Reads a network in CIDR form. Returns address and prefix.
pub fn parse_network(text: &str) -> Option<(IpAddr, u8)> {
    let (address, prefix) = text.split_once('/')?;
    let address: IpAddr = address.parse().ok()?;
    if prefix.is_empty() || !prefix.bytes().all(|byte| byte.is_ascii_digit()) || prefix.len() > 3 {
        return None;
    }
    let prefix: u8 = prefix.parse().ok()?;
    let max = if address.is_ipv4() { 32 } else { 128 };
    (prefix <= max).then_some((address, prefix))
}

/// Normalizes a fallback DNS server: without a port `:53`, IPv6 in square brackets. `None` for an
/// invalid entry or port 0.
pub fn normalize_dns_server(text: &str) -> Option<String> {
    let text = text.trim();
    if let Ok(address) = text.parse::<IpAddr>() {
        return Some(SocketAddr::new(address, 53).to_string());
    }
    let address: SocketAddr = text.parse().ok()?;
    (address.port() != 0).then(|| address.to_string())
}

/// DNS stamp: `sdns://`, then only Base64url characters, at most 1,024 characters in total.
pub fn is_valid_stamp(stamp: &str) -> bool {
    stamp.len() <= 1024
        && stamp.strip_prefix("sdns://").is_some_and(|rest| {
            !rest.is_empty()
                && rest
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        })
}

fn is_valid_interface(name: &str) -> bool {
    (1..=15).contains(&name.len())
        && name != "."
        && name != ".."
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

fn is_valid_backup(name: &str) -> bool {
    name.is_empty()
        || (name.len() <= 255
            && name != "."
            && name != ".."
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')))
}

/// Permitted range of the number fields.
fn range_of(field: Field) -> Option<(u64, u64)> {
    Some(match field {
        Field::Port => (1, 65535),
        Field::DpdSecs => (1, 60),
        Field::RestTimeoutSecs => (5, 90),
        Field::ReconnectWaitSecs => (1, 300),
        Field::ListenPort => (0, 65535),
        Field::Mark => (0, u64::from(u32::MAX)),
        Field::RoutingTable => (1, u64::from(u32::MAX)),
        Field::RulePriority => (1, 32765),
        _ => return None,
    })
}

fn range_error(field: Field) -> Phrase {
    let (low, high) = range_of(field).unwrap_or((0, 0));
    Phrase::fill(
        Key::FieldRange,
        [Phrase::raw(low.to_string()), Phrase::raw(high.to_string())],
    )
}

fn number_of(settings: &Settings, field: Field) -> Option<u64> {
    Some(match field {
        Field::Port => settings.port.into(),
        Field::DpdSecs => settings.dpd_secs.into(),
        Field::RestTimeoutSecs => settings.rest_timeout_secs.into(),
        Field::ReconnectWaitSecs => settings.reconnect_wait_secs.into(),
        Field::ListenPort => settings.listen_port.into(),
        Field::Mark => settings.mark.into(),
        Field::RoutingTable => settings.routing_table.into(),
        Field::RulePriority => settings.rule_priority.into(),
        _ => return None,
    })
}

/// Checks the whole set. The interface must not be named like an existing one; the caller removes the
/// name set so far from `env` beforehand.
pub fn validate(settings: &Settings, env: &Environment) -> Result<(), Vec<FieldError>> {
    let mut errors = Vec::new();
    let filter = &settings.filter;
    if !PG_LEVELS.contains(&filter.pg) {
        errors.push(FieldError::new(Field::Pg, Key::FieldPgError));
    }
    if filter.risk.iter().any(|risk| !RISK_LEVELS.contains(&risk.as_str())) {
        errors.push(FieldError::new(Field::Risk, Key::FieldRiskUnknown));
    }
    if filter
        .illegal
        .iter()
        .any(|kind| !ILLEGAL_KINDS.contains(&kind.as_str()))
    {
        errors.push(FieldError::new(Field::Illegal, Key::FieldIllegalUnknown));
    }
    if filter
        .categories
        .iter()
        .any(|name| !crate::categories::is_valid_name(name))
    {
        errors.push(FieldError::new(Field::Categories, Key::FieldCategoryUnknown));
    }
    for (field, list) in [
        (Field::Whitelist, &filter.whitelist),
        (Field::Blacklist, &filter.blacklist),
    ] {
        if list.iter().any(|name| !is_valid_filter_name(name)) {
            errors.push(FieldError::new(field, NAME_ERROR));
        }
    }
    if filter.whitelist.iter().any(|name| filter.blacklist.contains(name)) {
        errors.push(FieldError::new(Field::Blacklist, Key::FieldNameInBothLists));
    }
    if settings.plain_dns.is_empty() {
        errors.push(FieldError::new(Field::PlainDns, Key::FieldDnsRequired));
    } else if settings
        .plain_dns
        .iter()
        .any(|server| normalize_dns_server(server).as_deref() != Some(server))
    {
        errors.push(FieldError::new(Field::PlainDns, DNS_ERROR));
    }
    if settings
        .split_tunnel
        .iter()
        .any(|network| parse_network(network).is_none())
    {
        errors.push(FieldError::new(Field::SplitTunnel, NETWORK_ERROR));
    }
    if settings.doh_servers.iter().any(|stamp| !is_valid_stamp(stamp)) {
        errors.push(FieldError::new(Field::DohServers, STAMP_ERROR));
    }
    for field in Field::ALL {
        if let (Some(value), Some((low, high))) = (number_of(settings, field), range_of(field))
            && !(low..=high).contains(&value)
        {
            errors.push(FieldError::new(field, range_error(field)));
        }
    }
    if (253..=255).contains(&settings.routing_table) {
        errors.push(FieldError::new(Field::RoutingTable, TABLE_RESERVED));
    }
    if !is_valid_interface(&settings.interface) {
        errors.push(FieldError::new(Field::Interface, INTERFACE_ERROR));
    } else if env.interfaces.contains(&settings.interface) {
        errors.push(FieldError::new(Field::Interface, INTERFACE_TAKEN));
    }
    if !is_valid_backup(&settings.resolv_backup) {
        errors.push(FieldError::new(Field::ResolvBackup, BACKUP_ERROR));
    }
    if errors.is_empty() { Ok(()) } else { Err(errors) }
}

// ---------------------------------------------------------------------------------------------------
// JSON for the service
// ---------------------------------------------------------------------------------------------------

/// Go `time.Duration` counts nanoseconds.
fn nanos(secs: u32) -> u64 {
    u64::from(secs) * 1_000_000_000
}

/// Complete set for `POST /configuration`. Never contains credentials or fields such as `AccessToken`,
/// `AccessTokenPath`, `Username`, `Password`, `CA`, `Domain`, `PrivateKey`, `Control`, `NextHop`.
pub fn to_service_json(settings: &Settings, host: &str) -> String {
    let mut value = service_value(settings);
    value["Rest"]["Host"] = json!(host);
    value.to_string()
}

/// Like [`to_service_json`], but without `Rest.Host`: for `/route`, where no server is selected. The
/// service then keeps its previous host.
pub fn to_service_json_without_host(settings: &Settings) -> String {
    service_value(settings).to_string()
}

fn service_value(settings: &Settings) -> serde_json::Value {
    let filter = &settings.filter;
    let backup = if settings.resolv_backup.is_empty() {
        String::new()
    } else {
        format!("{BACKUP_DIR}/{}", settings.resolv_backup)
    };
    json!({
        "Rest": {
            "Port": settings.port,
            "RestTimeout": nanos(settings.rest_timeout_secs),
            "ReconnectWait": nanos(settings.reconnect_wait_secs),
            // The CLI sets both marks to the same value.
            "Mark": settings.mark,
            "UseDoH": settings.use_doh,
            "Filter": {
                "forceDns": filter.force_dns,
                "ads": filter.ads,
                "trackers": filter.trackers,
                "malware": filter.malware,
                "malicious": filter.malicious,
                "safeSearch": filter.safe_search,
                "PG": filter.pg,
                "risk": filter.risk,
                "illegal": filter.illegal,
                "whitelist": filter.whitelist,
                "blacklist": filter.blacklist,
                "categories": filter.categories,
            },
            "PortForward": { "enabled": settings.port_forward },
        },
        "WireGuard": {
            "Name": settings.interface,
            "ListenPort": settings.listen_port,
            "Mark": settings.mark,
            "RoutingTable": settings.routing_table,
            "RPDBPriority": settings.rule_priority,
            "LeakProtection": settings.kill_switch,
            "ResolvConfBackupFile": backup,
            "DpdTimeout": nanos(settings.dpd_secs),
            "SplitTunnel": settings.split_tunnel.join(","),
            "IPv4": settings.ip_version != IpVersion::V6Only,
            "IPv6": settings.ip_version != IpVersion::V4Only,
        },
        "DoH": { "servers": settings.doh_servers },
        "Plain": { "Servers": settings.plain_dns },
    })
}

// ---------------------------------------------------------------------------------------------------
// Saving and loading
// ---------------------------------------------------------------------------------------------------

/// Largest permitted settings file.
const MAX_FILE: u64 = 64 * 1024;
const FILE_VERSION: u32 = 1;

/// `$XDG_CONFIG_HOME/celata/einstellungen.json`, otherwise `~/.config/celata/einstellungen.json`.
pub fn config_path() -> Option<PathBuf> {
    Some(
        files::user_dir("XDG_CONFIG_HOME", ".config")?
            .join("celata")
            .join("einstellungen.json"),
    )
}

/// Appearance of the window: light (the default, also for the first start and for any unreadable value) or
/// dark. Only the user changes it, with the button in the header; Celata never follows the desktop.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Appearance {
    #[default]
    Light,
    Dark,
}

impl Appearance {
    /// The word in the file.
    pub fn code(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    pub fn is_dark(self) -> bool {
        self == Self::Dark
    }

    /// Only the word `dark` (ignoring spaces and case) means dark; any other text means light.
    fn from_text(text: &str) -> Self {
        if text.trim().eq_ignore_ascii_case("dark") {
            Self::Dark
        } else {
            Self::Light
        }
    }
}

/// Settings for Celata itself only. Never part of [`Settings`] and never sent to the service:
/// `to_service_json`, `changed_fields`, `reset_all` and `PendingChange` do not know this type.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(default)]
pub struct AppSettings {
    /// Show the donation hint in the sidebar (default: on).
    pub donation_hint: bool,
    /// Language of the user interface. In the file as a code (`"en"`, `"de"`); if it is missing or
    /// unknown, English applies without the loading failing.
    #[serde(serialize_with = "write_language", deserialize_with = "read_language")]
    pub language: Language,
    /// Appearance of the window. In the file `"light"` or `"dark"`; if it is missing or anything else, light
    /// applies without the loading failing and without touching the other settings.
    #[serde(serialize_with = "write_appearance", deserialize_with = "read_appearance")]
    pub appearance: Appearance,
    /// Favourite locations as host names, newest first (`favorites::toggle`). Read leniently: only valid,
    /// distinct host names count, at most [`MAX_FAVORITES`]; anything else is skipped without the loading
    /// failing.
    #[serde(deserialize_with = "read_favorites")]
    pub favorites: Vec<String>,
    /// Desktop notifications about the connection while the window is not in front (default: on).
    pub notifications: bool,
    /// Closing the window leaves Celata running with its icon in the system tray (default: off).
    pub keep_running: bool,
    /// Ask GitHub for a newer version at start and once a day (default: off; GitHub sees the public address
    /// without VPN).
    pub update_check: bool,
}

/// Most favourites that are read from the file. Keeps the file far below `MAX_FILE` (256 host names are
/// about 16 KiB); there are about 130 locations.
pub const MAX_FAVORITES: usize = 256;

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            donation_hint: true,
            language: Language::default(),
            appearance: Appearance::default(),
            favorites: Vec::new(),
            notifications: true,
            keep_running: false,
            update_check: false,
        }
    }
}

/// Accepts any JSON value; only a list counts. From it only strings that are valid host names after
/// trimming and lower case; the first of several equal ones stays; at most [`MAX_FAVORITES`].
fn read_favorites<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Vec<String>, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    let mut favorites: Vec<String> = Vec::new();
    for item in value.as_array().map(Vec::as_slice).unwrap_or_default() {
        if favorites.len() >= MAX_FAVORITES {
            break;
        }
        if let Some(host) = item.as_str().and_then(crate::favorites::normalize)
            && !favorites.contains(&host)
        {
            favorites.push(host);
        }
    }
    Ok(favorites)
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde calls `serialize_with` functions with a reference"
)]
fn write_language<S: serde::Serializer>(language: &Language, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(language.code())
}

/// Accepts any JSON value; only a known code counts, otherwise English.
fn read_language<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Language, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(value.as_str().and_then(Language::from_code).unwrap_or_default())
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde calls `serialize_with` functions with a reference"
)]
fn write_appearance<S: serde::Serializer>(appearance: &Appearance, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(appearance.code())
}

/// Accepts any JSON value; only the text `dark` counts as dark, everything else is light.
fn read_appearance<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Appearance, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(value.as_str().map(Appearance::from_text).unwrap_or_default())
}

#[derive(Serialize, Deserialize)]
struct SettingsFile {
    version: u32,
    settings: Settings,
    /// Missing in files of older versions; the defaults apply then.
    #[serde(default)]
    app: AppSettings,
}

#[derive(Deserialize)]
struct VersionOnly {
    version: u32,
}

/// Result of loading: the settings in force and, where applicable, a notice for the page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loaded {
    /// Always contains defaults for hidden settings ([`FILTERS_WORK`]).
    pub settings: Settings,
    /// Selection of the hidden settings as stored in the file. It has no effect anywhere and is only
    /// written back when saving ([`save`]).
    pub kept: Filter,
    /// Settings for Celata only; defaults if the file is missing or unreadable.
    pub app: AppSettings,
    pub notice: Option<Key>,
}

pub const BROKEN_NOTICE: Key = Key::SettingsFileBroken;
pub const NEWER_NOTICE: Key = Key::SettingsFileNewer;

/// Reads the settings. If the file is missing, defaults apply without a notice. If it is damaged, too
/// large, invalid or from a newer version, defaults apply with a notice; the file stays.
pub fn load(path: Option<&Path>) -> Loaded {
    load_with(path, FILTERS_WORK)
}

fn load_with(path: Option<&Path>, filters_work: bool) -> Loaded {
    let standard = |notice: Option<Key>| Loaded {
        settings: Settings::default(),
        kept: Filter::default(),
        app: AppSettings::default(),
        notice,
    };
    let fallback = |notice: Key| standard(Some(notice));
    let Some(path) = path else {
        return standard(None);
    };
    let mut content = Vec::new();
    match fs::File::open(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return standard(None);
        }
        Err(_) => return fallback(BROKEN_NOTICE),
        Ok(file) => {
            if file.take(MAX_FILE + 1).read_to_end(&mut content).is_err() || content.len() as u64 > MAX_FILE {
                return fallback(BROKEN_NOTICE);
            }
        }
    }
    match serde_json::from_slice::<VersionOnly>(&content) {
        Ok(VersionOnly { version }) if version > FILE_VERSION => return fallback(NEWER_NOTICE),
        Ok(VersionOnly { version: FILE_VERSION }) => {}
        _ => return fallback(BROKEN_NOTICE),
    }
    match serde_json::from_slice::<SettingsFile>(&content) {
        // The stored interface name is Celata's own and may exist. The whole file is checked, including the
        // values that are held back afterwards.
        Ok(file) if validate(&file.settings, &Environment::default()).is_ok() => {
            let (settings, kept) = split(file.settings, filters_work);
            Loaded {
                settings,
                kept,
                app: file.app,
                notice: None,
            }
        }
        _ => fallback(BROKEN_NOTICE),
    }
}

/// Permissions of the settings file and the applied state: for the user only.
const PRIVATE_MODE: u32 = 0o600;

/// Saves the settings (file with `version: 1`, permissions 0600). `kept` is the held-back selection of
/// hidden settings from [`load`]; it stays in the file that way. `app` is a separate entry next to it.
pub fn save(path: &Path, settings: &Settings, kept: &Filter, app: &AppSettings) -> io::Result<()> {
    save_with(path, settings, kept, app, FILTERS_WORK)
}

fn save_with(path: &Path, settings: &Settings, kept: &Filter, app: &AppSettings, filters_work: bool) -> io::Result<()> {
    let file = SettingsFile {
        version: FILE_VERSION,
        settings: merge(settings, kept, filters_work),
        app: app.clone(),
    };
    let content = serde_json::to_vec_pretty(&file).map_err(io::Error::other)?;
    // What could not be loaded again is not written; the previous file stays.
    if u64::try_from(content.len()).map_or(true, |length| length > MAX_FILE) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the settings file would exceed 64 KiB",
        ));
    }
    files::write_atomic(path, &content, PRIVATE_MODE)
}

// ---------------------------------------------------------------------------------------------------
// Applied state
// ---------------------------------------------------------------------------------------------------

/// What Celata last sent to which run of the service.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
#[serde(default)]
pub struct Applied {
    /// `InvocationID` of the service at the last send, `None` = unknown.
    pub invocation: Option<String>,
    /// Settings last sent to this run; `None` = never (defaults apply then).
    pub sent: Option<Settings>,
    /// Settings the routes were last built with; `None` = unknown.
    pub routes_built_with: Option<Settings>,
}

/// `~/.cache/celata/uebernahme.json`.
pub fn applied_path() -> Option<PathBuf> {
    crate::servers::cache_path().map(|path| path.with_file_name("uebernahme.json"))
}

/// Reads the applied state; missing or damaged means unknown.
pub fn load_applied(path: Option<&Path>) -> Applied {
    let Some(path) = path else { return Applied::default() };
    let mut content = Vec::new();
    let read = fs::File::open(path).and_then(|file| file.take(4 * MAX_FILE).read_to_end(&mut content));
    match read {
        Ok(_) => serde_json::from_slice(&content).unwrap_or_default(),
        Err(_) => Applied::default(),
    }
}

/// Saves the applied state (permissions 0600).
pub fn save_applied(path: &Path, applied: &Applied) -> io::Result<()> {
    let content = serde_json::to_vec(applied).map_err(io::Error::other)?;
    files::write_atomic(path, &content, PRIVATE_MODE)
}

/// Checks the output of `systemctl show -p InvocationID --value`: exactly 32 hex characters.
pub fn parse_invocation(output: &str) -> Option<String> {
    let id = output.trim();
    (id.len() == 32 && id.bytes().all(|byte| byte.is_ascii_hexdigit())).then(|| id.to_ascii_lowercase())
}

const SYSTEMCTL_COMMANDS: [&str; 2] = ["/usr/bin/systemctl", "/bin/systemctl"];
/// Largest output of `systemctl` that is read; 32 characters are expected.
const MAX_SYSTEMCTL_OUTPUT: u64 = 4096;

/// `InvocationID` of the running service; changes with every start. Readable without root.
pub fn invocation_id() -> Option<String> {
    let args = ["show", "-p", "InvocationID", "--value", crate::state::SERVICE_UNIT];
    let output = crate::process::run(&SYSTEMCTL_COMMANDS, &args, MAX_SYSTEMCTL_OUTPUT).ok()?;
    parse_invocation(&output)
}

/// Is it possible that `applied` went to the same run of the service as `current`? If one of the two
/// is unknown, Celata has to assume so out of caution.
fn maybe_same_run(applied: &Applied, current: Option<&str>) -> bool {
    match (applied.invocation.as_deref(), current) {
        (Some(before), Some(now)) => before == now,
        _ => true,
    }
}

/// Does the full set have to be sent? False only if the settings are at their defaults and nothing
/// else was ever sent in this run of the service.
pub fn send_full(applied: &Applied, settings: &Settings, current: Option<&str>) -> bool {
    if !settings.is_default() {
        return true;
    }
    maybe_same_run(applied, current) && applied.sent.as_ref().is_some_and(|sent| !sent.is_default())
}

/// Do "Rebuild" values differ from the last time the routes were built? In another run of the service
/// the service built them with defaults. If the state is unknown: rebuild as soon as a "Rebuild" value
/// is not at its default.
pub fn needs_rebuild(applied: &Applied, settings: &Settings, current: Option<&str>) -> bool {
    let rebuild = |base: &Settings| changed_class(base, settings) == Some(Class::Rebuild);
    let default = Settings::default();
    match (applied.invocation.as_deref(), current, &applied.routes_built_with) {
        (Some(before), Some(now), Some(built)) if before == now => rebuild(built),
        _ => rebuild(&default),
    }
}

impl Applied {
    /// After an accepted `POST /configuration`. `full`: the full set went out.
    pub fn record_sent(&mut self, settings: &Settings, full: bool, current: Option<&str>) {
        if !maybe_same_run(self, current) {
            self.routes_built_with = None;
        }
        self.invocation = current.map(str::to_owned);
        self.sent = Some(if full { settings.clone() } else { Settings::default() });
    }

    /// After a connection that created the routes anew (`/connect` from `clean`, after `/destroy`).
    pub fn record_built(&mut self, settings: &Settings) {
        self.routes_built_with = Some(settings.clone());
    }
}

/// Applied state together with the `InvocationID` of the running service as read before an operation.
/// The sequences in `state.rs` and `switch.rs` record here what they sent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ledger {
    pub applied: Applied,
    pub current: Option<String>,
}

impl Ledger {
    pub fn new(applied: Applied, current: Option<String>) -> Self {
        Self { applied, current }
    }

    pub fn send_full(&self, settings: &Settings) -> bool {
        send_full(&self.applied, settings, self.current.as_deref())
    }

    pub fn needs_rebuild(&self, settings: &Settings) -> bool {
        needs_rebuild(&self.applied, settings, self.current.as_deref())
    }

    pub fn record_sent(&mut self, settings: &Settings, full: bool) {
        let current = self.current.clone();
        self.applied.record_sent(settings, full, current.as_deref());
    }

    pub fn record_built(&mut self, settings: &Settings) {
        self.applied.record_built(settings);
    }
}

/// Notice when non-default settings went to an untested version of the service. `{0}` is the tested
/// version [`crate::state::EXPECTED_VERSION`].
pub const VERSION_NOTICE: Key = Key::SettingsVersionNotice;

/// Notice for [`VERSION_NOTICE`] with the version the service reports.
pub fn untested_notice(version: &str) -> Phrase {
    Phrase::fill(
        Key::UntestedVersion,
        [
            Phrase::fill(VERSION_NOTICE, [Phrase::raw(crate::state::EXPECTED_VERSION)]),
            Phrase::raw(version),
        ],
    )
}

/// What the page shows about the state of the settings.
pub fn state_text(applied: &Applied, settings: &Settings, current: Option<&str>, connected: bool) -> Key {
    // Compare without hidden values: hiding alone is not a change.
    let sent_matches = maybe_same_run(applied, current)
        && applied
            .sent
            .as_ref()
            .map_or(settings.is_default(), |sent| visible(sent) == *settings);
    if connected && sent_matches && !needs_rebuild(applied, settings, current) {
        Key::SettingsAppliesNow
    } else {
        Key::SettingsAppliesNext
    }
}

// ---------------------------------------------------------------------------------------------------
// Changes from the user interface
// ---------------------------------------------------------------------------------------------------

/// A change from the user interface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edit {
    /// Switch toggled.
    Toggle(String, bool),
    /// Selection: parental control, IP version; for risk, illegal and category the entry is toggled.
    Choice(String, String),
    /// Text field committed.
    Text(String, String),
    ListAdd(String, String),
    ListRemove(String, usize),
    /// "all" or "advanced".
    Reset(String),
}

fn unknown(key: &str) -> Vec<FieldError> {
    // Unknown keys only come from a bug in the user interface; the error is shown at the filter.
    vec![FieldError::new(
        Field::Ads,
        Phrase::fill(Key::FieldUnknownSetting, [Phrase::raw(key)]),
    )]
}

fn toggle_set(set: &mut BTreeSet<String>, value: &str) {
    if !set.remove(value) {
        set.insert(value.to_owned());
    }
}

/// Applies a change and checks the result. On an error the stored set stays as it is.
/// `known_categories`: names from the loaded category list (only these can be selected).
pub fn apply_edit(
    old: &Settings,
    edit: Edit,
    env: &Environment,
    known_categories: &[String],
) -> Result<Settings, Vec<FieldError>> {
    let mut new = old.clone();
    let error = |field: Field, message: Key| Err(vec![FieldError::new(field, message)]);
    match edit {
        // One switch for all kinds of illegal content (clearer than four separate ones).
        Edit::Toggle(key, on) if key == "illegal" => {
            new.filter.illegal = if on {
                ILLEGAL_KINDS.iter().map(|kind| (*kind).to_owned()).collect()
            } else {
                BTreeSet::new()
            };
        }
        Edit::Choice(key, value) if key == "risk_level" => match value.parse::<u8>().ok().and_then(risk_for_level) {
            Some(risk) => new.filter.risk = risk,
            None => return error(Field::Risk, Key::FieldRiskUnknown),
        },
        Edit::Toggle(key, on) => {
            let target = match Field::from_key(&key) {
                Some(Field::Ads) => &mut new.filter.ads,
                Some(Field::Trackers) => &mut new.filter.trackers,
                Some(Field::Malware) => &mut new.filter.malware,
                Some(Field::Malicious) => &mut new.filter.malicious,
                Some(Field::SafeSearch) => &mut new.filter.safe_search,
                Some(Field::ForceDns) => &mut new.filter.force_dns,
                Some(Field::UseDoh) => &mut new.use_doh,
                Some(Field::PortForward) => &mut new.port_forward,
                Some(Field::KillSwitch) => &mut new.kill_switch,
                _ => return Err(unknown(&key)),
            };
            *target = on;
        }
        Edit::Choice(key, value) => match Field::from_key(&key) {
            Some(Field::Pg) => match value.parse::<u8>() {
                Ok(level) if PG_LEVELS.contains(&level) => new.filter.pg = level,
                _ => return error(Field::Pg, Key::FieldPgError),
            },
            Some(Field::IpVersion) => match IpVersion::from_key(&value) {
                Some(version) => new.ip_version = version,
                None => return error(Field::IpVersion, Key::FieldIpVersionUnknown),
            },
            Some(Field::Risk) if RISK_LEVELS.contains(&value.as_str()) => toggle_set(&mut new.filter.risk, &value),
            Some(Field::Illegal) if ILLEGAL_KINDS.contains(&value.as_str()) => {
                toggle_set(&mut new.filter.illegal, &value);
            }
            // Deselecting always works, even if the list is not loaded at the moment.
            Some(Field::Categories) if new.filter.categories.contains(&value) || known_categories.contains(&value) => {
                toggle_set(&mut new.filter.categories, &value);
            }
            Some(field @ (Field::Risk | Field::Illegal | Field::Categories)) => {
                return error(field, Key::FieldNotSelectable);
            }
            _ => return Err(unknown(&key)),
        },
        Edit::Text(key, value) => {
            let value = value.trim();
            match Field::from_key(&key) {
                Some(Field::Interface) => value.clone_into(&mut new.interface),
                Some(Field::ResolvBackup) => value.clone_into(&mut new.resolv_backup),
                Some(field) if range_of(field).is_some() => {
                    let number = value
                        .parse::<u64>()
                        .ok()
                        .filter(|number| range_of(field).is_some_and(|(low, high)| (low..=high).contains(number)));
                    let Some(number) = number else {
                        return Err(vec![FieldError::new(field, range_error(field))]);
                    };
                    // The ranges fit into the field types; `try_from` cannot fail here.
                    let narrow = |number: u64| u32::try_from(number).unwrap_or(u32::MAX);
                    match field {
                        Field::Port => new.port = u16::try_from(number).unwrap_or(u16::MAX),
                        Field::ListenPort => new.listen_port = u16::try_from(number).unwrap_or(u16::MAX),
                        Field::DpdSecs => new.dpd_secs = narrow(number),
                        Field::RestTimeoutSecs => new.rest_timeout_secs = narrow(number),
                        Field::ReconnectWaitSecs => new.reconnect_wait_secs = narrow(number),
                        Field::Mark => new.mark = narrow(number),
                        Field::RoutingTable => new.routing_table = narrow(number),
                        _ => new.rule_priority = narrow(number),
                    }
                }
                _ => return Err(unknown(&key)),
            }
        }
        Edit::ListAdd(key, value) => {
            let value = value.trim();
            let field = Field::from_key(&key);
            let (list, entry) = match field {
                Some(Field::Whitelist) => (&mut new.filter.whitelist, value.to_owned()),
                Some(Field::Blacklist) => (&mut new.filter.blacklist, value.to_owned()),
                Some(Field::SplitTunnel) => (&mut new.split_tunnel, value.to_owned()),
                Some(Field::DohServers) => (&mut new.doh_servers, value.to_owned()),
                Some(Field::PlainDns) => match normalize_dns_server(value) {
                    Some(server) => (&mut new.plain_dns, server),
                    None => return error(Field::PlainDns, DNS_ERROR),
                },
                _ => return Err(unknown(&key)),
            };
            // Duplicate entries are merged.
            if !list.contains(&entry) {
                list.push(entry);
            }
        }
        Edit::ListRemove(key, index) => {
            let list = match Field::from_key(&key) {
                Some(Field::Whitelist) => &mut new.filter.whitelist,
                Some(Field::Blacklist) => &mut new.filter.blacklist,
                Some(Field::SplitTunnel) => &mut new.split_tunnel,
                Some(Field::DohServers) => &mut new.doh_servers,
                Some(Field::PlainDns) => &mut new.plain_dns,
                _ => return Err(unknown(&key)),
            };
            if index < list.len() {
                list.remove(index);
            }
        }
        Edit::Reset(section) => match section.as_str() {
            "all" => new = Settings::default(),
            "advanced" => {
                let default = Settings::default();
                new.ip_version = default.ip_version;
                new.port = default.port;
                new.dpd_secs = default.dpd_secs;
                new.rest_timeout_secs = default.rest_timeout_secs;
                new.reconnect_wait_secs = default.reconnect_wait_secs;
                new.interface = default.interface;
                new.listen_port = default.listen_port;
                new.mark = default.mark;
                new.routing_table = default.routing_table;
                new.rule_priority = default.rule_priority;
                new.resolv_backup = default.resolv_backup;
                new.doh_servers = default.doh_servers;
            }
            _ => return Err(unknown(&section)),
        },
    }
    validate(&new, &env.without(&old.interface))?;
    Ok(new)
}

// ---------------------------------------------------------------------------------------------------
// Bundling, warnings and hints
// ---------------------------------------------------------------------------------------------------

/// Waiting time after the last change before reconnecting.
pub const BUNDLE_DELAY: Duration = Duration::from_secs(2);

/// Changes made while a connection exists that are not applied yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingChange {
    /// Time of the last change.
    pub since: Instant,
    /// Settings before the first change of the bundle (for the way back).
    pub old: Settings,
}

impl PendingChange {
    pub fn new(old: Settings, now: Instant) -> Self {
        Self { since: now, old }
    }

    /// Another change extends the deadline; `old` stays the state before the bundle.
    pub fn extend(&mut self, now: Instant) {
        self.since = now;
    }

    /// 2 seconds after the last change.
    pub fn due(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.since) >= BUNDLE_DELAY
    }
}

/// Warning before a change is applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    pub field: Option<Field>,
    pub text: Phrase,
}

// Texts of the warnings: `src/i18n`, keys `Warning…`.
pub const REBUILD_WARNING: Key = Key::WarningRebuild;
pub const KILL_SWITCH_WARNING: Key = Key::WarningKillSwitch;
pub const PORT_FORWARD_WARNING: Key = Key::WarningPortForward;
pub const SPLIT_WARNING: Key = Key::WarningSplit;
pub const WIDE_SPLIT_WARNING: Key = Key::WarningWideSplit;
pub const RESOLVER_WARNING: Key = Key::WarningResolver;
pub const DOH_OFF_WARNING: Key = Key::WarningDohOff;
pub const ADVANCED_WARNING: Key = Key::WarningAdvanced;

/// Is the network so large that almost all traffic bypasses the VPN?
pub fn is_wide_network(network: &str) -> bool {
    match parse_network(network) {
        Some((IpAddr::V4(_), prefix)) => prefix <= 8,
        Some((IpAddr::V6(_), prefix)) => prefix <= 16,
        None => false,
    }
}

/// Does the field in `settings` reduce the protection?
fn reduces_protection(field: Field, settings: &Settings) -> bool {
    let default = Settings::default();
    match field {
        Field::UseDoh => !settings.use_doh,
        Field::PlainDns => settings.plain_dns != default.plain_dns,
        Field::SplitTunnel => !settings.split_tunnel.is_empty(),
        Field::PortForward => settings.port_forward,
        Field::KillSwitch => !settings.kill_switch,
        Field::ResolvBackup => !settings.resolv_backup.is_empty(),
        field if field.is_advanced() => field.differs(settings, &default),
        _ => false,
    }
}

/// Warnings before applying: one for every field that newly reduces the protection, and for "Rebuild"
/// during a connection also the short gap. Switching the kill switch back on does not warn.
pub fn warnings(old: &Settings, new: &Settings, connected: bool) -> Vec<Warning> {
    let mut list = Vec::new();
    for field in changed_fields(old, new) {
        if !reduces_protection(field, new) {
            continue;
        }
        let text = match field {
            Field::KillSwitch => Phrase::key(KILL_SWITCH_WARNING),
            Field::PortForward => Phrase::key(PORT_FORWARD_WARNING),
            Field::SplitTunnel => {
                let added: Vec<&String> = new
                    .split_tunnel
                    .iter()
                    .filter(|network| !old.split_tunnel.contains(network))
                    .collect();
                if added.is_empty() {
                    continue;
                }
                if added.iter().any(|network| is_wide_network(network)) {
                    Phrase::fill(
                        Key::SentencePair,
                        [Phrase::key(WIDE_SPLIT_WARNING), Phrase::key(SPLIT_WARNING)],
                    )
                } else {
                    Phrase::key(SPLIT_WARNING)
                }
            }
            Field::UseDoh => Phrase::key(DOH_OFF_WARNING),
            Field::PlainDns => Phrase::key(RESOLVER_WARNING),
            Field::ResolvBackup => Phrase::key(Key::WarningResolvBackup),
            _ => Phrase::key(ADVANCED_WARNING),
        };
        if !list.iter().any(|warning: &Warning| warning.text == text) {
            list.push(Warning {
                field: Some(field),
                text,
            });
        }
    }
    // If only the kill switch is switched back on, the internet was unprotected before anyway: no question.
    let only_kill_switch_on = new.kill_switch
        && changed_fields(old, new)
            .into_iter()
            .all(|field| class_of(field) == Class::Reconnect || field == Field::KillSwitch);
    if connected && changed_class(old, new) == Some(Class::Rebuild) && !only_kill_switch_on {
        list.push(Warning {
            field: None,
            text: Phrase::key(REBUILD_WARNING),
        });
    }
    list
}

/// Permanent hint for the overview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hint {
    pub text: Phrase,
    /// Red: kill switch off, DNS leak possible.
    pub alert: bool,
}

/// Hints for all settings in force that reduce the protection. Before them comes, as a warning, the DNS
/// notice for as long as `dns` reports a possible DNS leak; it does not depend on any setting.
pub fn active_hints(settings: &Settings, dns: Assessment) -> Vec<Hint> {
    let plain = |text: Phrase| Hint { text, alert: false };
    let mut hints = Vec::new();
    if dns == Assessment::AtRisk {
        hints.push(Hint {
            text: Phrase::key(Key::HintDnsLeak),
            alert: true,
        });
    }
    if !settings.kill_switch {
        hints.push(Hint {
            text: Phrase::key(Key::HintKillSwitchOff),
            alert: true,
        });
    }
    if !settings.split_tunnel.is_empty() {
        hints.push(plain(Phrase::fill(
            Key::HintSplitTunnel,
            [Phrase::raw(settings.split_tunnel.join(", "))],
        )));
    }
    if settings.port_forward {
        hints.push(plain(Phrase::key(Key::HintPortForward)));
    }
    if !settings.use_doh {
        hints.push(plain(Phrase::key(Key::HintDohOff)));
    }
    let default = Settings::default();
    if settings.plain_dns != default.plain_dns {
        hints.push(plain(Phrase::fill(
            Key::HintPlainDns,
            [Phrase::raw(settings.plain_dns.join(", "))],
        )));
    }
    if Field::ALL
        .into_iter()
        .any(|field| field.is_advanced() && field.differs(settings, &default))
    {
        hints.push(plain(Phrase::key(Key::HintAdvanced)));
    }
    hints
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Deutsch;

    fn env(names: &[&str]) -> Environment {
        Environment {
            interfaces: names.iter().map(|name| (*name).to_owned()).collect(),
        }
    }

    fn edit(settings: &Settings, edit: Edit) -> Result<Settings, Vec<FieldError>> {
        apply_edit(settings, edit, &env(&["lo", "eth0", "vpn"]), &["gambling".to_owned()])
    }

    fn text(key: &str, value: &str) -> Edit {
        Edit::Text(key.to_owned(), value.to_owned())
    }

    fn add(key: &str, value: &str) -> Edit {
        Edit::ListAdd(key.to_owned(), value.to_owned())
    }

    fn fails(settings: &Settings, change: Edit, field: Field) {
        let shown = format!("{change:?}");
        let errors = edit(settings, change).expect_err(&format!("{shown} müsste scheitern"));
        assert!(errors.iter().any(|error| error.field == field), "{shown}: {errors:?}");
    }

    #[test]
    fn standardwerte_entsprechen_v0_9_14() {
        let settings = Settings::default();
        assert!(settings.is_default());
        assert!(settings.use_doh && settings.kill_switch && !settings.port_forward);
        assert_eq!(settings.port, 432);
        assert_eq!(settings.dpd_secs, 60);
        assert_eq!(settings.rest_timeout_secs, 90);
        assert_eq!(settings.reconnect_wait_secs, 30);
        assert_eq!(settings.interface, "vpn");
        assert_eq!((settings.listen_port, settings.mark), (0, 0));
        assert_eq!(settings.routing_table, 55555);
        assert_eq!(settings.rule_priority, 10);
        assert_eq!(settings.plain_dns, ["209.250.251.37:53", "217.182.206.81:53"]);
        assert_eq!(settings.doh_servers.len(), 18);
        assert!(settings.doh_servers.iter().all(|stamp| is_valid_stamp(stamp)));
        assert_eq!(settings.filter, Filter::default());
        assert_eq!(settings.ip_version, IpVersion::Both);
        assert!(validate(&settings, &env(&["lo", "eth0"])).is_ok());
    }

    #[test]
    fn jedes_feld_hat_einen_schluessel() {
        for field in Field::ALL {
            assert_eq!(Field::from_key(field.key()), Some(field));
        }
        assert_eq!(Field::from_key("unbekannt"), None);
    }

    #[test]
    fn klassen_nach_der_tabelle() {
        let rebuild = [
            Field::KillSwitch,
            Field::IpVersion,
            Field::RestTimeoutSecs,
            Field::Interface,
            Field::ListenPort,
            Field::Mark,
            Field::RoutingTable,
            Field::RulePriority,
            Field::DohServers,
        ];
        for field in Field::ALL {
            let expected = if rebuild.contains(&field) {
                Class::Rebuild
            } else {
                Class::Reconnect
            };
            assert_eq!(class_of(field), expected, "{field:?}");
        }
    }

    #[test]
    fn jede_feldaenderung_ergibt_ihre_klasse() {
        let base = Settings::default();
        let changes: Vec<(Field, Settings)> = vec![
            (
                Field::Ads,
                Settings {
                    filter: Filter {
                        ads: true,
                        ..Filter::default()
                    },
                    ..base.clone()
                },
            ),
            (
                Field::Pg,
                Settings {
                    filter: Filter {
                        pg: 18,
                        ..Filter::default()
                    },
                    ..base.clone()
                },
            ),
            (
                Field::ForceDns,
                Settings {
                    filter: Filter {
                        force_dns: true,
                        ..Filter::default()
                    },
                    ..base.clone()
                },
            ),
            (
                Field::UseDoh,
                Settings {
                    use_doh: false,
                    ..base.clone()
                },
            ),
            (
                Field::PlainDns,
                Settings {
                    plain_dns: vec!["9.9.9.9:53".to_owned()],
                    ..base.clone()
                },
            ),
            (
                Field::SplitTunnel,
                Settings {
                    split_tunnel: vec!["192.168.1.0/24".to_owned()],
                    ..base.clone()
                },
            ),
            (
                Field::PortForward,
                Settings {
                    port_forward: true,
                    ..base.clone()
                },
            ),
            (
                Field::KillSwitch,
                Settings {
                    kill_switch: false,
                    ..base.clone()
                },
            ),
            (
                Field::IpVersion,
                Settings {
                    ip_version: IpVersion::V4Only,
                    ..base.clone()
                },
            ),
            (
                Field::Port,
                Settings {
                    port: 433,
                    ..base.clone()
                },
            ),
            (
                Field::DpdSecs,
                Settings {
                    dpd_secs: 30,
                    ..base.clone()
                },
            ),
            (
                Field::RestTimeoutSecs,
                Settings {
                    rest_timeout_secs: 30,
                    ..base.clone()
                },
            ),
            (
                Field::ReconnectWaitSecs,
                Settings {
                    reconnect_wait_secs: 5,
                    ..base.clone()
                },
            ),
            (
                Field::Interface,
                Settings {
                    interface: "tw0".to_owned(),
                    ..base.clone()
                },
            ),
            (
                Field::ListenPort,
                Settings {
                    listen_port: 51820,
                    ..base.clone()
                },
            ),
            (
                Field::Mark,
                Settings {
                    mark: 7,
                    ..base.clone()
                },
            ),
            (
                Field::RoutingTable,
                Settings {
                    routing_table: 100,
                    ..base.clone()
                },
            ),
            (
                Field::RulePriority,
                Settings {
                    rule_priority: 20,
                    ..base.clone()
                },
            ),
            (
                Field::ResolvBackup,
                Settings {
                    resolv_backup: "resolv.bak".to_owned(),
                    ..base.clone()
                },
            ),
            (
                Field::DohServers,
                Settings {
                    doh_servers: Vec::new(),
                    ..base.clone()
                },
            ),
        ];
        for (field, changed) in changes {
            assert_eq!(changed_fields(&base, &changed), vec![field]);
            assert_eq!(changed_class(&base, &changed), Some(class_of(field)), "{field:?}");
        }
        assert_eq!(changed_class(&base, &base), None);
    }

    #[test]
    fn neuaufbau_hat_vorrang_vor_neu_verbinden() {
        let base = Settings::default();
        let both = Settings {
            filter: Filter {
                ads: true,
                ..Filter::default()
            },
            kill_switch: false,
            ..base.clone()
        };
        assert_eq!(changed_class(&base, &both), Some(Class::Rebuild));
    }

    // --- Validation rules (at least 20 invalid and 10 valid cases) ---

    #[test]
    fn ungueltige_eingaben_werden_abgelehnt() {
        let base = Settings::default();
        fails(&base, add("whitelist", "bad name"), Field::Whitelist);
        fails(&base, add("blacklist", "ex/ample.com"), Field::Blacklist);
        fails(&base, add("whitelist", ""), Field::Whitelist);
        fails(&base, add("whitelist", &"a".repeat(254)), Field::Whitelist);
        fails(&base, add("split_tunnel", "192.168.1.0/33"), Field::SplitTunnel);
        fails(&base, add("split_tunnel", "192.168.1.0"), Field::SplitTunnel);
        fails(&base, add("split_tunnel", "300.1.1.1/24"), Field::SplitTunnel);
        fails(&base, add("split_tunnel", "2001:db8::/129"), Field::SplitTunnel);
        fails(&base, add("split_tunnel", "192.168.1.0/+8"), Field::SplitTunnel);
        fails(&base, add("plain_dns", "dns.example"), Field::PlainDns);
        fails(&base, add("plain_dns", "9.9.9.9:0"), Field::PlainDns);
        fails(&base, add("plain_dns", "9.9.9.9:70000"), Field::PlainDns);
        fails(
            &base,
            add("doh_servers", "https://dns.example/dns-query"),
            Field::DohServers,
        );
        fails(&base, add("doh_servers", "sdns://abc+def"), Field::DohServers);
        fails(
            &base,
            add("doh_servers", &format!("sdns://{}", "A".repeat(1020))),
            Field::DohServers,
        );
        fails(&base, text("port", "0"), Field::Port);
        fails(&base, text("port", "zwölf"), Field::Port);
        fails(&base, text("dpd_secs", "61"), Field::DpdSecs);
        fails(&base, text("rest_timeout_secs", "4"), Field::RestTimeoutSecs);
        fails(&base, text("reconnect_wait_secs", "301"), Field::ReconnectWaitSecs);
        fails(&base, text("routing_table", "254"), Field::RoutingTable);
        fails(&base, text("routing_table", "0"), Field::RoutingTable);
        fails(&base, text("rule_priority", "32766"), Field::RulePriority);
        fails(&base, text("mark", "4294967296"), Field::Mark);
        fails(&base, text("listen_port", "-1"), Field::ListenPort);
        fails(&base, text("interface", "eth0"), Field::Interface);
        fails(&base, text("interface", "viel-zu-langer-name"), Field::Interface);
        fails(&base, text("interface", "a b"), Field::Interface);
        fails(&base, text("interface", ".."), Field::Interface);
        fails(&base, text("resolv_backup", "../passwd"), Field::ResolvBackup);
        fails(&base, text("resolv_backup", "/etc/passwd"), Field::ResolvBackup);
        fails(&base, text("resolv_backup", ".."), Field::ResolvBackup);
        fails(&base, Edit::Choice("pg".to_owned(), "16".to_owned()), Field::Pg);
        fails(
            &base,
            Edit::Choice("risk".to_owned(), "extreme".to_owned()),
            Field::Risk,
        );
        fails(
            &base,
            Edit::Choice("category".to_owned(), "erfunden".to_owned()),
            Field::Categories,
        );
    }

    #[test]
    fn name_darf_nicht_in_beiden_listen_stehen() {
        let base = edit(&Settings::default(), add("whitelist", "example.com")).unwrap();
        fails(&base, add("blacklist", "example.com"), Field::Blacklist);
    }

    #[test]
    fn letzter_dns_server_bleibt() {
        let one = Settings {
            plain_dns: vec!["9.9.9.9:53".to_owned()],
            ..Settings::default()
        };
        fails(&one, Edit::ListRemove("plain_dns".to_owned(), 0), Field::PlainDns);
    }

    #[test]
    fn gueltige_eingaben_werden_uebernommen() {
        let base = Settings::default();
        assert_eq!(
            edit(&base, add("whitelist", "*.example.com")).unwrap().filter.whitelist,
            ["*.example.com"]
        );
        assert_eq!(
            edit(&base, add("blacklist", "doubleclick.net"))
                .unwrap()
                .filter
                .blacklist,
            ["doubleclick.net"]
        );
        assert_eq!(
            edit(&base, add("split_tunnel", "192.168.1.0/24")).unwrap().split_tunnel,
            ["192.168.1.0/24"]
        );
        assert_eq!(
            edit(&base, add("split_tunnel", "2001:db8::/32")).unwrap().split_tunnel,
            ["2001:db8::/32"]
        );
        assert_eq!(
            edit(&base, add("plain_dns", "9.9.9.9"))
                .unwrap()
                .plain_dns
                .last()
                .unwrap(),
            "9.9.9.9:53"
        );
        assert_eq!(
            edit(&base, add("plain_dns", "2620:fe::fe"))
                .unwrap()
                .plain_dns
                .last()
                .unwrap(),
            "[2620:fe::fe]:53"
        );
        assert_eq!(
            edit(&base, add("plain_dns", "1.1.1.1:5353"))
                .unwrap()
                .plain_dns
                .last()
                .unwrap(),
            "1.1.1.1:5353"
        );
        assert_eq!(edit(&base, text("port", " 433 ")).unwrap().port, 433);
        assert_eq!(
            edit(&base, text("routing_table", "4294967295")).unwrap().routing_table,
            u32::MAX
        );
        assert_eq!(
            edit(&base, text("rule_priority", "32765")).unwrap().rule_priority,
            32765
        );
        assert_eq!(edit(&base, text("mark", "4294967295")).unwrap().mark, u32::MAX);
        assert_eq!(edit(&base, text("interface", "tw0")).unwrap().interface, "tw0");
        // The name set so far may exist.
        assert_eq!(edit(&base, text("interface", "vpn")).unwrap().interface, "vpn");
        assert_eq!(
            edit(&base, text("resolv_backup", "resolv.conf.bak"))
                .unwrap()
                .resolv_backup,
            "resolv.conf.bak"
        );
        assert_eq!(edit(&base, text("resolv_backup", "")).unwrap().resolv_backup, "");
        assert_eq!(
            edit(&base, Edit::Choice("pg".to_owned(), "21".to_owned()))
                .unwrap()
                .filter
                .pg,
            21
        );
        assert_eq!(
            edit(&base, add("doh_servers", "sdns://AgMAAA_-"))
                .unwrap()
                .doh_servers
                .len(),
            19
        );
    }

    #[test]
    fn doppelte_eintraege_werden_zusammengefasst() {
        let once = edit(&Settings::default(), add("whitelist", "example.com")).unwrap();
        let twice = edit(&once, add("whitelist", " example.com ")).unwrap();
        assert_eq!(twice.filter.whitelist, ["example.com"]);
    }

    #[test]
    fn auswahl_schaltet_eintraege_um() {
        let base = Settings::default();
        let risk = Edit::Choice("risk".to_owned(), "high".to_owned());
        let on = edit(&base, risk.clone()).unwrap();
        assert!(on.filter.risk.contains("high"));
        assert!(edit(&on, risk).unwrap().filter.risk.is_empty());
        let category = edit(&base, Edit::Choice("category".to_owned(), "gambling".to_owned())).unwrap();
        assert!(category.filter.categories.contains("gambling"));
        let ip = edit(&base, Edit::Choice("ip_version".to_owned(), "v6".to_owned())).unwrap();
        assert_eq!(ip.ip_version, IpVersion::V6Only);
    }

    #[test]
    fn risikostufen_und_illegale_inhalte() {
        let base = Settings::default();
        let level = |value: &str| edit(&base, Edit::Choice("risk_level".to_owned(), value.to_owned()));
        assert!(level("0").unwrap().filter.risk.is_empty());
        assert_eq!(level("1").unwrap().filter.risk, ["high".to_owned()].into());
        assert_eq!(
            level("2").unwrap().filter.risk,
            ["high".to_owned(), "medium".to_owned()].into()
        );
        assert_eq!(level("3").unwrap().filter.risk.len(), 3);
        assert!(level("4").is_err());
        for value in 0..=3 {
            assert_eq!(risk_level(&risk_for_level(value).unwrap()), value);
        }
        assert_eq!(risk_level(&["medium".to_owned()].into()), 2);
        let illegal = edit(&base, Edit::Toggle("illegal".to_owned(), true)).unwrap();
        assert_eq!(illegal.filter.illegal.len(), 4);
        assert!(
            edit(&illegal, Edit::Toggle("illegal".to_owned(), false))
                .unwrap()
                .filter
                .illegal
                .is_empty()
        );
    }

    #[test]
    fn schalter_und_entfernen() {
        let base = Settings::default();
        let ads = edit(&base, Edit::Toggle("ads".to_owned(), true)).unwrap();
        assert!(ads.filter.ads);
        assert!(
            !edit(&base, Edit::Toggle("kill_switch".to_owned(), false))
                .unwrap()
                .kill_switch
        );
        let list = edit(&base, add("whitelist", "a.example")).unwrap();
        let list = edit(&list, add("whitelist", "b.example")).unwrap();
        assert_eq!(
            edit(&list, Edit::ListRemove("whitelist".to_owned(), 0))
                .unwrap()
                .filter
                .whitelist,
            ["b.example"]
        );
        assert_eq!(edit(&list, Edit::ListRemove("whitelist".to_owned(), 9)).unwrap(), list);
    }

    #[test]
    fn unbekannte_schluessel_sind_fehler() {
        let base = Settings::default();
        assert!(edit(&base, Edit::Toggle("lab_preview".to_owned(), true)).is_err());
        assert!(edit(&base, Edit::Toggle("port".to_owned(), true)).is_err());
        assert!(edit(&base, Edit::Choice("ads".to_owned(), "1".to_owned())).is_err());
        assert!(edit(&base, text("ads", "1")).is_err());
        assert!(edit(&base, add("port", "1")).is_err());
        assert!(edit(&base, Edit::ListRemove("ads".to_owned(), 0)).is_err());
        assert!(edit(&base, Edit::Reset("filter".to_owned())).is_err());
    }

    #[test]
    fn zuruecksetzen_alles_und_erweitert() {
        let base = Settings::default();
        let changed = edit(&base, Edit::Toggle("ads".to_owned(), true)).unwrap();
        let changed = edit(&changed, text("port", "433")).unwrap();
        let advanced = edit(&changed, Edit::Reset("advanced".to_owned())).unwrap();
        assert!(advanced.filter.ads);
        assert_eq!(advanced.port, 432);
        assert!(edit(&changed, Edit::Reset("all".to_owned())).unwrap().is_default());
    }

    // --- JSON for the service ---

    fn service_json(settings: &Settings) -> serde_json::Value {
        serde_json::from_str(&to_service_json(settings, "nl.hideservers.net")).unwrap()
    }

    #[test]
    fn standard_json_entspricht_configuration_go() {
        let value = service_json(&Settings::default());
        let rest = &value["Rest"];
        assert_eq!(rest["Host"], "nl.hideservers.net");
        assert_eq!(rest["Port"], 432);
        assert_eq!(rest["RestTimeout"], 90_000_000_000u64);
        assert_eq!(rest["ReconnectWait"], 30_000_000_000u64);
        assert_eq!(rest["Mark"], 0);
        assert_eq!(rest["UseDoH"], true);
        assert_eq!(rest["PortForward"]["enabled"], false);
        let filter = &rest["Filter"];
        for key in ["forceDns", "ads", "trackers", "malware", "malicious", "safeSearch"] {
            assert_eq!(filter[key], false, "{key}");
        }
        assert_eq!(filter["PG"], 0);
        for key in ["risk", "illegal", "whitelist", "blacklist", "categories"] {
            assert_eq!(filter[key], serde_json::json!([]), "{key}");
        }
        let wireguard = &value["WireGuard"];
        assert_eq!(wireguard["Name"], "vpn");
        assert_eq!(wireguard["ListenPort"], 0);
        assert_eq!(wireguard["Mark"], 0);
        assert_eq!(wireguard["RoutingTable"], 55555);
        assert_eq!(wireguard["RPDBPriority"], 10);
        assert_eq!(wireguard["LeakProtection"], true);
        assert_eq!(wireguard["ResolvConfBackupFile"], "");
        assert_eq!(wireguard["DpdTimeout"], 60_000_000_000u64);
        assert_eq!(wireguard["SplitTunnel"], "");
        assert_eq!(wireguard["IPv4"], true);
        assert_eq!(wireguard["IPv6"], true);
        assert_eq!(value["DoH"]["servers"].as_array().unwrap().len(), 18);
        assert_eq!(
            value["Plain"]["Servers"],
            serde_json::json!(["209.250.251.37:53", "217.182.206.81:53"])
        );
    }

    #[test]
    fn abweichende_werte_im_json() {
        let settings = Settings {
            split_tunnel: vec!["192.168.1.0/24".to_owned(), "10.0.0.0/8".to_owned()],
            ip_version: IpVersion::V4Only,
            mark: 42,
            resolv_backup: "resolv.bak".to_owned(),
            dpd_secs: 15,
            filter: Filter {
                pg: 12,
                risk: ["high".to_owned()].into(),
                ..Filter::default()
            },
            ..Settings::default()
        };
        let value = service_json(&settings);
        assert_eq!(value["WireGuard"]["SplitTunnel"], "192.168.1.0/24,10.0.0.0/8");
        assert_eq!(value["WireGuard"]["IPv4"], true);
        assert_eq!(value["WireGuard"]["IPv6"], false);
        assert_eq!(value["WireGuard"]["Mark"], value["Rest"]["Mark"]);
        assert_eq!(value["Rest"]["Mark"], 42);
        assert_eq!(value["WireGuard"]["ResolvConfBackupFile"], "/run/hide.me/resolv.bak");
        assert_eq!(value["WireGuard"]["DpdTimeout"], 15_000_000_000u64);
        assert_eq!(value["Rest"]["Filter"]["PG"], 12);
        assert_eq!(value["Rest"]["Filter"]["risk"], serde_json::json!(["high"]));
        let v6 = service_json(&Settings {
            ip_version: IpVersion::V6Only,
            ..Settings::default()
        });
        assert_eq!(
            (v6["WireGuard"]["IPv4"].as_bool(), v6["WireGuard"]["IPv6"].as_bool()),
            (Some(false), Some(true))
        );
    }

    #[test]
    fn json_ohne_host_fuer_route() {
        let value: serde_json::Value =
            serde_json::from_str(&to_service_json_without_host(&Settings::default())).unwrap();
        assert!(value["Rest"].get("Host").is_none());
        assert_eq!(value["WireGuard"]["LeakProtection"], true);
        assert_eq!(service_json(&Settings::default())["Rest"]["Host"], "nl.hideservers.net");
    }

    #[test]
    fn json_enthaelt_keine_verbotenen_felder() {
        let settings = Settings {
            filter: Filter {
                ads: true,
                ..Filter::default()
            },
            ..Settings::default()
        };
        let json = to_service_json(&settings, "nl.hideservers.net").to_ascii_lowercase();
        for forbidden in [
            "accesstoken",
            "username",
            "password",
            "\"ca\"",
            "domain",
            "privatekey",
            "control",
            "nexthop",
        ] {
            assert!(!json.contains(forbidden), "{forbidden}");
        }
    }

    // --- Saving and loading ---

    fn temp_file(name: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let number = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("celata-einstellungen-{}-{number}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn rundweg_speichern_und_laden() {
        let path = temp_file("einstellungen.json");
        let settings = Settings {
            filter: Filter {
                ads: true,
                whitelist: vec!["example.com".to_owned()],
                ..Filter::default()
            },
            port: 433,
            ..Settings::default()
        };
        // With working filters, saving and loading behave as they did before the filters were hidden.
        save_with(&path, &settings, &Filter::default(), &AppSettings::default(), true).unwrap();
        assert_eq!(
            load_with(Some(&path), true),
            Loaded {
                settings,
                kept: Filter::default(),
                app: AppSettings::default(),
                notice: None
            }
        );
        let mode = fs::metadata(&path).unwrap().permissions();
        assert_eq!(std::os::unix::fs::PermissionsExt::mode(&mode) & 0o777, 0o600);
    }

    #[test]
    fn fehlende_datei_ergibt_standard_ohne_hinweis() {
        let path = temp_file("fehlt.json");
        let empty = Loaded {
            settings: Settings::default(),
            kept: Filter::default(),
            app: AppSettings::default(),
            notice: None,
        };
        assert_eq!(load(Some(&path)), empty);
        assert_eq!(load(None), empty);
    }

    #[test]
    fn beschaedigte_datei_ergibt_standard_mit_hinweis_und_bleibt() {
        let path = temp_file("einstellungen.json");
        fs::write(&path, b"{ halb geschrieben").unwrap();
        let loaded = load(Some(&path));
        assert!(loaded.settings.is_default());
        assert_eq!(loaded.notice, Some(BROKEN_NOTICE));
        assert_eq!(fs::read(&path).unwrap(), b"{ halb geschrieben");
    }

    #[test]
    fn ungueltige_werte_in_der_datei_ergeben_standard() {
        let path = temp_file("einstellungen.json");
        fs::write(&path, br#"{"version":1,"settings":{"routing_table":254}}"#).unwrap();
        assert_eq!(load(Some(&path)).notice, Some(BROKEN_NOTICE));
    }

    #[test]
    fn neuere_version_ergibt_standard_mit_hinweis() {
        let path = temp_file("einstellungen.json");
        fs::write(&path, br#"{"version":2,"settings":{}}"#).unwrap();
        let loaded = load(Some(&path));
        assert!(loaded.settings.is_default());
        assert_eq!(loaded.notice, Some(NEWER_NOTICE));
    }

    #[test]
    fn zu_grosse_datei_wird_nicht_gelesen() {
        let path = temp_file("einstellungen.json");
        fs::write(&path, vec![b' '; 64 * 1024 + 1]).unwrap();
        assert_eq!(load(Some(&path)).notice, Some(BROKEN_NOTICE));
    }

    #[test]
    fn fehlende_felder_bekommen_standardwerte() {
        let path = temp_file("einstellungen.json");
        fs::write(&path, br#"{"version":1,"settings":{"filter":{"ads":true}}}"#).unwrap();
        let loaded = load_with(Some(&path), true);
        assert_eq!(loaded.notice, None);
        assert!(loaded.settings.filter.ads);
        assert_eq!(loaded.settings.port, 432);
    }

    // --- Applied state ---

    const RUN_A: &str = "0123456789abcdef0123456789abcdef";
    const RUN_B: &str = "fedcba9876543210fedcba9876543210";

    fn ads() -> Settings {
        Settings {
            filter: Filter {
                ads: true,
                ..Filter::default()
            },
            ..Settings::default()
        }
    }

    fn no_kill_switch() -> Settings {
        Settings {
            kill_switch: false,
            ..Settings::default()
        }
    }

    #[test]
    fn invocation_id_wird_geprueft() {
        assert_eq!(
            parse_invocation(&format!("{}\n", RUN_A.to_uppercase())).as_deref(),
            Some(RUN_A)
        );
        assert_eq!(parse_invocation(""), None);
        assert_eq!(parse_invocation("1234"), None);
        assert_eq!(parse_invocation(&"g".repeat(32)), None);
    }

    #[test]
    fn standard_ohne_frueheres_senden_schickt_nur_den_host() {
        let none = Applied::default();
        assert!(!send_full(&none, &Settings::default(), Some(RUN_A)));
        assert!(!send_full(&none, &Settings::default(), None));
        let default_sent = Applied {
            invocation: Some(RUN_A.to_owned()),
            sent: Some(Settings::default()),
            routes_built_with: None,
        };
        assert!(!send_full(&default_sent, &Settings::default(), Some(RUN_A)));
    }

    #[test]
    fn abweichende_einstellungen_schicken_immer_den_vollen_satz() {
        assert!(send_full(&Applied::default(), &ads(), Some(RUN_A)));
        assert!(send_full(&Applied::default(), &ads(), None));
    }

    #[test]
    fn zurueck_auf_standard_im_selben_lauf_schickt_den_vollen_satz() {
        let applied = Applied {
            invocation: Some(RUN_A.to_owned()),
            sent: Some(ads()),
            routes_built_with: None,
        };
        assert!(send_full(&applied, &Settings::default(), Some(RUN_A)));
        // Run unknown: cautious, like the same run.
        assert!(send_full(&applied, &Settings::default(), None));
        let unknown = Applied {
            invocation: None,
            ..applied.clone()
        };
        assert!(send_full(&unknown, &Settings::default(), Some(RUN_A)));
        // New run: the service has forgotten everything.
        assert!(!send_full(&applied, &Settings::default(), Some(RUN_B)));
    }

    #[test]
    fn neuaufbau_nach_dem_letzten_aufbau() {
        let built = |settings: Settings| Applied {
            invocation: Some(RUN_A.to_owned()),
            sent: Some(settings.clone()),
            routes_built_with: Some(settings),
        };
        // Same run, same values: nothing to do. Only "Reconnect" values changed: nothing either.
        assert!(!needs_rebuild(&built(no_kill_switch()), &no_kill_switch(), Some(RUN_A)));
        assert!(!needs_rebuild(&built(Settings::default()), &ads(), Some(RUN_A)));
        // Same run, kill switch changed: rebuild.
        assert!(needs_rebuild(
            &built(Settings::default()),
            &no_kill_switch(),
            Some(RUN_A)
        ));
        assert!(needs_rebuild(
            &built(no_kill_switch()),
            &Settings::default(),
            Some(RUN_A)
        ));
        // Different run: comparison with the defaults.
        assert!(!needs_rebuild(
            &built(no_kill_switch()),
            &Settings::default(),
            Some(RUN_B)
        ));
        assert!(needs_rebuild(&built(no_kill_switch()), &no_kill_switch(), Some(RUN_B)));
        // Unknown: true as soon as a "Rebuild" value is not at its default.
        assert!(needs_rebuild(&Applied::default(), &no_kill_switch(), None));
        assert!(!needs_rebuild(&Applied::default(), &ads(), None));
        assert!(needs_rebuild(&built(no_kill_switch()), &no_kill_switch(), None));
        let no_routes = Applied {
            routes_built_with: None,
            ..built(no_kill_switch())
        };
        assert!(needs_rebuild(&no_routes, &no_kill_switch(), Some(RUN_A)));
    }

    #[test]
    fn senden_und_aufbauen_werden_vermerkt() {
        let mut applied = Applied::default();
        applied.record_sent(&ads(), true, Some(RUN_A));
        assert_eq!(applied.sent, Some(ads()));
        applied.record_built(&ads());
        assert_eq!(applied.routes_built_with, Some(ads()));
        // New run: the old build no longer counts.
        applied.record_sent(&Settings::default(), false, Some(RUN_B));
        assert_eq!(applied.routes_built_with, None);
        assert_eq!(applied.sent, Some(Settings::default()));
        assert_eq!(applied.invocation.as_deref(), Some(RUN_B));
    }

    #[test]
    fn stand_der_einstellungen() {
        let split = Settings {
            split_tunnel: vec!["192.168.1.0/24".to_owned()],
            ..Settings::default()
        };
        let applied = Applied {
            invocation: Some(RUN_A.to_owned()),
            sent: Some(split.clone()),
            routes_built_with: Some(Settings::default()),
        };
        assert_eq!(
            state_text(&applied, &split, Some(RUN_A), true).de(),
            "Gilt für die laufende Verbindung."
        );
        assert_eq!(
            state_text(&applied, &split, Some(RUN_A), false).de(),
            "Gilt ab der nächsten Verbindung."
        );
        assert_eq!(
            state_text(&applied, &Settings::default(), Some(RUN_A), true).de(),
            "Gilt ab der nächsten Verbindung."
        );
        assert_eq!(
            state_text(&Applied::default(), &Settings::default(), Some(RUN_A), true).de(),
            "Gilt für die laufende Verbindung."
        );
    }

    #[test]
    fn uebernahmestand_rundweg() {
        let path = temp_file("uebernahme.json");
        let applied = Applied {
            invocation: Some(RUN_A.to_owned()),
            sent: Some(ads()),
            routes_built_with: None,
        };
        save_applied(&path, &applied).unwrap();
        assert_eq!(load_applied(Some(&path)), applied);
        fs::write(&path, b"kaputt").unwrap();
        assert_eq!(load_applied(Some(&path)), Applied::default());
    }

    // --- Bundling, warnings, hints ---

    #[test]
    fn buendel_ist_zwei_sekunden_nach_der_letzten_aenderung_faellig() {
        let start = Instant::now();
        let mut pending = PendingChange::new(Settings::default(), start);
        assert!(!pending.due(start + Duration::from_millis(1999)));
        pending.extend(start + Duration::from_secs(1));
        assert!(!pending.due(start + Duration::from_millis(2500)));
        assert!(pending.due(start + Duration::from_secs(3)));
        assert!(pending.old.is_default());
    }

    #[test]
    fn filter_einschalten_warnt_nicht() {
        assert!(warnings(&Settings::default(), &ads(), true).is_empty());
    }

    #[test]
    fn sperre_aus_warnt_mit_luecke_bei_verbindung() {
        let list = warnings(&Settings::default(), &no_kill_switch(), true);
        let texts: Vec<Phrase> = list.iter().map(|warning| warning.text.clone()).collect();
        assert_eq!(texts, [Phrase::key(KILL_SWITCH_WARNING), Phrase::key(REBUILD_WARNING)]);
        let offline = warnings(&Settings::default(), &no_kill_switch(), false);
        assert_eq!(offline.len(), 1);
    }

    #[test]
    fn sperre_wieder_an_ohne_rueckfrage() {
        assert!(warnings(&no_kill_switch(), &Settings::default(), true).is_empty());
        assert!(warnings(&no_kill_switch(), &Settings::default(), false).is_empty());
        // If another "Rebuild" value changes as well, the warning about the gap stays.
        let with_port = Settings {
            listen_port: 51820,
            ..Settings::default()
        };
        let texts: Vec<Phrase> = warnings(&no_kill_switch(), &with_port, true)
            .into_iter()
            .map(|warning| warning.text)
            .collect();
        assert!(texts.contains(&Phrase::key(REBUILD_WARNING)));
    }

    #[test]
    fn schutzverringernde_werte_warnen() {
        let base = Settings::default();
        let cases = [
            (
                Settings {
                    port_forward: true,
                    ..base.clone()
                },
                PORT_FORWARD_WARNING,
            ),
            (
                Settings {
                    use_doh: false,
                    ..base.clone()
                },
                DOH_OFF_WARNING,
            ),
            (
                Settings {
                    plain_dns: vec!["9.9.9.9:53".to_owned()],
                    ..base.clone()
                },
                RESOLVER_WARNING,
            ),
            (
                Settings {
                    split_tunnel: vec!["192.168.1.0/24".to_owned()],
                    ..base.clone()
                },
                SPLIT_WARNING,
            ),
            (
                Settings {
                    port: 433,
                    ..base.clone()
                },
                ADVANCED_WARNING,
            ),
        ];
        for (settings, text) in cases {
            let list = warnings(&base, &settings, false);
            assert_eq!(
                list.first().map(|warning| warning.text.clone()),
                Some(Phrase::key(text))
            );
        }
    }

    #[test]
    fn grosses_netz_braucht_eigene_bestaetigung() {
        assert!(is_wide_network("0.0.0.0/0"));
        assert!(is_wide_network("10.0.0.0/8"));
        assert!(!is_wide_network("10.0.0.0/9"));
        assert!(is_wide_network("2000::/3"));
        assert!(!is_wide_network("2001:db8::/32"));
        let wide = Settings {
            split_tunnel: vec!["10.0.0.0/8".to_owned()],
            ..Settings::default()
        };
        let list = warnings(&Settings::default(), &wide, false);
        assert!(list[0].text.de().starts_with(&WIDE_SPLIT_WARNING.de()));
        assert!(list[0].text.de().ends_with(&SPLIT_WARNING.de()));
    }

    #[test]
    fn netz_entfernen_warnt_nicht() {
        let two = Settings {
            split_tunnel: vec!["192.168.1.0/24".to_owned(), "10.1.0.0/16".to_owned()],
            ..Settings::default()
        };
        let one = Settings {
            split_tunnel: vec!["192.168.1.0/24".to_owned()],
            ..Settings::default()
        };
        assert!(warnings(&two, &one, true).is_empty());
    }

    #[test]
    fn hinweise_fuer_die_uebersicht() {
        assert!(active_hints(&Settings::default(), Assessment::Clear).is_empty());
        let hints = active_hints(&no_kill_switch(), Assessment::Clear);
        assert_eq!(hints.len(), 1);
        assert!(hints[0].alert);
        let split = Settings {
            split_tunnel: vec!["192.168.1.0/24".to_owned()],
            ..Settings::default()
        };
        let hints = active_hints(&split, Assessment::Clear);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].text.de(), "Split-Tunnel aktiv: 192.168.1.0/24");
        assert!(!hints[0].alert);
        let advanced = Settings {
            port: 433,
            port_forward: true,
            ..Settings::default()
        };
        let hints = active_hints(&advanced, Assessment::Clear);
        assert_eq!(hints.len(), 2);
        assert!(hints.iter().all(|hint| !hint.alert));
        // Filters no longer produce a hint.
        assert!(active_hints(&ads(), Assessment::Clear).is_empty());
        assert!(
            active_hints(&mit_filtern(), Assessment::Clear)
                .iter()
                .all(|hint| !hint.text.de().contains("Filter"))
        );
    }

    #[test]
    fn dns_hint_comes_first_as_a_warning_and_only_at_risk() {
        let dns = Hint {
            text: Phrase::key(Key::HintDnsLeak),
            alert: true,
        };
        let cases = [
            Settings::default(),
            no_kill_switch(),
            Settings {
                port: 433,
                port_forward: true,
                ..Settings::default()
            },
        ];
        for settings in cases {
            let usual = active_hints(&settings, Assessment::Clear);
            assert_eq!(
                active_hints(&settings, Assessment::Unknown),
                usual,
                "unknown must not change the list"
            );
            let at_risk = active_hints(&settings, Assessment::AtRisk);
            assert_eq!(at_risk.first(), Some(&dns));
            assert_eq!(at_risk[1..], usual[..], "the other hints stay as they are");
        }
        assert!(!usual_texts_mention_dns_leak());
    }

    /// Without the risk, no hint speaks about a DNS leak, whatever the settings are.
    fn usual_texts_mention_dns_leak() -> bool {
        let everything = Settings {
            kill_switch: false,
            port_forward: true,
            use_doh: false,
            port: 433,
            ..Settings::default()
        };
        active_hints(&everything, Assessment::Clear)
            .iter()
            .any(|hint| hint.text == Phrase::key(Key::HintDnsLeak))
    }

    // --- Hidden settings ---

    /// Settings as an older version stored them, with filters.
    fn mit_filtern() -> Settings {
        Settings {
            filter: Filter {
                ads: true,
                pg: 18,
                whitelist: vec!["example.com".to_owned()],
                force_dns: true,
                ..Filter::default()
            },
            split_tunnel: vec!["192.168.1.0/24".to_owned()],
            ..Settings::default()
        }
    }

    fn ohne_filter() -> Settings {
        Settings {
            filter: Filter::default(),
            ..mit_filtern()
        }
    }

    /// Writes a file in the older format (the filters are part of the settings themselves).
    fn alte_datei(settings: &Settings) -> PathBuf {
        let path = temp_file("einstellungen.json");
        save_with(&path, settings, &Filter::default(), &AppSettings::default(), true).unwrap();
        path
    }

    #[test]
    fn laden_haelt_ausgeblendete_filter_zurueck() {
        let path = alte_datei(&mit_filtern());
        let loaded = load(Some(&path));
        assert_eq!(loaded.notice, None);
        assert_eq!(loaded.settings, ohne_filter());
        assert!(loaded.settings.filter.is_empty());
        assert_eq!(loaded.kept, mit_filtern().filter);
    }

    #[test]
    fn laden_und_speichern_erhaelt_die_ausgeblendete_auswahl() {
        let path = alte_datei(&mit_filtern());
        let loaded = load(Some(&path));
        save(&path, &loaded.settings, &loaded.kept, &loaded.app).unwrap();
        assert_eq!(load(Some(&path)), loaded);
        // The selection is still in the file as before.
        assert_eq!(load_with(Some(&path), true).settings, mit_filtern());
        assert!(fs::read_to_string(&path).unwrap().contains("\"ads\": true"));
    }

    #[test]
    fn mit_wirksamen_filtern_gibt_es_nichts_zurueckzuhalten() {
        let path = alte_datei(&mit_filtern());
        let loaded = load_with(Some(&path), true);
        assert_eq!(loaded.settings, mit_filtern());
        assert!(loaded.kept.is_empty());
        // `kept` plays no role when saving then.
        save_with(
            &path,
            &ohne_filter(),
            &mit_filtern().filter,
            &AppSettings::default(),
            true,
        )
        .unwrap();
        assert_eq!(load_with(Some(&path), true).settings, ohne_filter());
    }

    #[test]
    fn dienst_bekommt_keine_ausgeblendeten_filter() {
        let path = alte_datei(&mit_filtern());
        let json = to_service_json(&load(Some(&path)).settings, "nl.hideservers.net");
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        let filter = &value["Rest"]["Filter"];
        assert_eq!(filter["ads"], false);
        assert_eq!(filter["forceDns"], false);
        assert_eq!(filter["PG"], 0);
        assert_eq!(filter["whitelist"], serde_json::json!([]));
        assert_eq!(value["WireGuard"]["SplitTunnel"], "192.168.1.0/24");
        assert!(!json.contains("true,\"trackers\"") && !json.contains("example.com"));
    }

    #[test]
    fn ausblenden_allein_aendert_den_stand_nicht() {
        // The set last went to the service with filters; it is loaded without filters.
        let path = alte_datei(&mit_filtern());
        let loaded = load(Some(&path));
        let sent = mit_filtern();
        assert_eq!(visible(&sent), loaded.settings);
        // No change, so no reconnecting …
        assert_eq!(changed_class(&visible(&sent), &loaded.settings), None);
        // … and no "Applies from the next connection."
        let applied = Applied {
            invocation: Some(RUN_A.to_owned()),
            sent: Some(sent),
            routes_built_with: Some(Settings::default()),
        };
        assert_eq!(
            state_text(&applied, &loaded.settings, Some(RUN_A), true).de(),
            "Gilt für die laufende Verbindung."
        );
    }

    #[test]
    fn frueher_geschickte_filter_werden_im_dienst_abgeraeumt() {
        // The same run of the service already received filters: send the full set once.
        let applied = Applied {
            invocation: Some(RUN_A.to_owned()),
            sent: Some(ads()),
            routes_built_with: None,
        };
        assert!(send_full(&applied, &Settings::default(), Some(RUN_A)));
        // Different run: the service no longer knows the filters, the host is enough.
        assert!(!send_full(&applied, &Settings::default(), Some(RUN_B)));
    }

    #[test]
    fn ungueltiger_filterwert_in_der_datei_ergibt_weiter_standard() {
        let path = temp_file("einstellungen.json");
        fs::write(&path, br#"{"version":1,"settings":{"filter":{"pg":13}}}"#).unwrap();
        let loaded = load(Some(&path));
        assert_eq!(loaded.notice, Some(BROKEN_NOTICE));
        assert!(loaded.settings.is_default() && loaded.kept.is_empty());
    }

    #[test]
    fn zuruecksetzen_loescht_auch_ausgeblendete_einstellungen() {
        let empty = Filter::default();
        let kept = mit_filtern().filter;
        // Nothing to do: no question.
        assert_eq!(reset_all(&Settings::default(), &empty), None);
        // Only a hidden selection is stored: reset anyway.
        assert_eq!(
            reset_all(&Settings::default(), &kept),
            Some((Settings::default(), Filter::default()))
        );
        // Only a visible setting changed.
        assert_eq!(
            reset_all(&ohne_filter(), &empty),
            Some((Settings::default(), Filter::default()))
        );
        // The result saved: the file no longer contains filters.
        let path = alte_datei(&mit_filtern());
        let loaded = load(Some(&path));
        let (settings, kept) = reset_all(&loaded.settings, &loaded.kept).unwrap();
        save(&path, &settings, &kept, &loaded.app).unwrap();
        let again = load(Some(&path));
        assert!(again.settings.is_default() && again.kept.is_empty());
        assert!(load_with(Some(&path), true).settings.is_default());
        assert!(!fs::read_to_string(&path).unwrap().contains("\"ads\": true"));
    }

    // --- Switch "Show donation hint" ---

    fn ohne_spendenhinweis() -> AppSettings {
        AppSettings {
            donation_hint: false,
            ..AppSettings::default()
        }
    }

    #[test]
    fn datei_aus_funktion_006_zeigt_den_spendenhinweis() {
        // Form of the file in older versions: only `version` and `settings`.
        let path = temp_file("einstellungen.json");
        fs::write(
            &path,
            serde_json::to_vec(&serde_json::json!({"version": 1, "settings": mit_filtern()})).unwrap(),
        )
        .unwrap();
        let loaded = load(Some(&path));
        assert!(loaded.app.donation_hint);
        assert_eq!(loaded.settings, ohne_filter());
        assert_eq!(loaded.kept, mit_filtern().filter);
        assert_eq!(loaded.notice, None);
    }

    #[test]
    fn leerer_eintrag_app_zeigt_den_spendenhinweis() {
        let path = temp_file("einstellungen.json");
        fs::write(&path, br#"{"version":1,"settings":{},"app":{}}"#).unwrap();
        let loaded = load(Some(&path));
        assert!(loaded.app.donation_hint);
        assert_eq!(loaded.notice, None);
    }

    #[test]
    fn ausgeschalteter_spendenhinweis_bleibt_gespeichert() {
        let path = alte_datei(&mit_filtern());
        let loaded = load(Some(&path));
        save(&path, &loaded.settings, &loaded.kept, &ohne_spendenhinweis()).unwrap();
        let again = load(Some(&path));
        assert!(!again.app.donation_hint);
        assert_eq!(again.settings, loaded.settings);
        assert_eq!(again.kept, loaded.kept);
        let mode = fs::metadata(&path).unwrap().permissions();
        assert_eq!(std::os::unix::fs::PermissionsExt::mode(&mode) & 0o777, 0o600);
    }

    #[test]
    fn falscher_wert_fuer_den_spendenhinweis_gilt_als_beschaedigt() {
        let path = temp_file("einstellungen.json");
        fs::write(&path, br#"{"version":1,"settings":{},"app":{"donation_hint":"nein"}}"#).unwrap();
        let loaded = load(Some(&path));
        assert_eq!(loaded.notice, Some(BROKEN_NOTICE));
        assert!(loaded.settings.is_default());
        assert!(loaded.app.donation_hint);
    }

    #[test]
    fn ohne_datei_oder_bei_neuerer_version_ist_der_spendenhinweis_an() {
        assert!(load(Some(&temp_file("fehlt.json"))).app.donation_hint);
        let path = temp_file("einstellungen.json");
        fs::write(&path, br#"{"version":2,"settings":{},"app":{"donation_hint":false}}"#).unwrap();
        let loaded = load(Some(&path));
        assert_eq!(loaded.notice, Some(NEWER_NOTICE));
        assert!(loaded.app.donation_hint);
    }

    #[test]
    fn zuruecksetzen_laesst_den_spendenhinweis_stehen() {
        let path = alte_datei(&mit_filtern());
        let loaded = load(Some(&path));
        save(&path, &loaded.settings, &loaded.kept, &ohne_spendenhinweis()).unwrap();
        let loaded = load(Some(&path));
        let (settings, kept) = reset_all(&loaded.settings, &loaded.kept).unwrap();
        save(&path, &settings, &kept, &loaded.app).unwrap();
        let again = load(Some(&path));
        assert!(again.settings.is_default() && again.kept.is_empty());
        assert!(!again.app.donation_hint);
    }

    // --- Switches of the section "Application" ---

    #[test]
    fn neue_schalter_haben_ihre_standardwerte() {
        let app = AppSettings::default();
        assert!(app.notifications);
        assert!(!app.keep_running);
        assert!(!app.update_check);
        for loaded in [
            app_aus("{}"),
            app_aus(r#"{"donation_hint":false,"language":"de","favorites":["berlin.hideservers.net"]}"#),
        ] {
            assert!(loaded.app.notifications);
            assert!(!loaded.app.keep_running);
            assert!(!loaded.app.update_check);
            assert_eq!(loaded.notice, None);
        }
        assert!(load(Some(&temp_file("fehlt.json"))).app.notifications);
    }

    #[test]
    fn neue_schalter_bleiben_gespeichert_und_lassen_den_rest_stehen() {
        let path = alte_datei(&mit_filtern());
        let loaded = load(Some(&path));
        let app = AppSettings {
            donation_hint: false,
            language: Language::De,
            appearance: Appearance::Dark,
            favorites: vec!["berlin.hideservers.net".into(), "best".into()],
            notifications: false,
            keep_running: true,
            update_check: true,
        };
        save(&path, &loaded.settings, &loaded.kept, &app).unwrap();
        let again = load(Some(&path));
        assert_eq!(again.app, app);
        assert_eq!(again.settings, loaded.settings);
        assert_eq!(again.kept, loaded.kept);
        let file: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(file["version"], 1);
        assert_eq!(file["app"]["keep_running"], true);
        assert_eq!(file["app"]["update_check"], true);
        assert_eq!(file["app"]["notifications"], false);
        assert!(file["settings"].get("keep_running").is_none());
    }

    #[test]
    fn einzelne_neue_schalter_aus_der_datei() {
        let loaded = app_aus(r#"{"keep_running":true}"#);
        assert!(loaded.app.keep_running && loaded.app.notifications && !loaded.app.update_check);
        let loaded = app_aus(r#"{"notifications":false,"update_check":true}"#);
        assert!(!loaded.app.notifications && loaded.app.update_check && !loaded.app.keep_running);
        assert!(loaded.app.donation_hint);
    }

    // --- Stored language ---

    fn app_aus(app: &str) -> Loaded {
        let path = temp_file("einstellungen.json");
        let content = format!(
            r#"{{"version":1,"settings":{},"app":{app}}}"#,
            serde_json::to_string(&ohne_filter()).unwrap()
        );
        fs::write(&path, content).unwrap();
        load(Some(&path))
    }

    #[test]
    fn ohne_sprache_gilt_englisch() {
        assert_eq!(app_aus("{}").app.language, Language::En);
        assert_eq!(app_aus(r#"{"donation_hint":false}"#).app.language, Language::En);
        let path = temp_file("einstellungen.json");
        fs::write(&path, br#"{"version":1,"settings":{}}"#).unwrap();
        assert_eq!(load(Some(&path)).app.language, Language::En);
        assert_eq!(load(Some(&temp_file("fehlt.json"))).app.language, Language::En);
    }

    #[test]
    fn bekannte_sprache_wird_gelesen() {
        assert_eq!(app_aus(r#"{"language":"de"}"#).app.language, Language::De);
        assert_eq!(app_aus(r#"{"language":"DE"}"#).app.language, Language::De);
        assert_eq!(app_aus(r#"{"language":" de "}"#).app.language, Language::De);
    }

    #[test]
    fn unbekannte_sprache_gilt_als_englisch_und_der_rest_bleibt() {
        for value in [r#""xx""#, r#""""#, "42", "null", "[]", r#"{"a":1}"#] {
            let loaded = app_aus(&format!(r#"{{"donation_hint":false,"language":{value}}}"#));
            assert_eq!(loaded.app.language, Language::En, "{value}");
            assert!(!loaded.app.donation_hint, "{value}");
            assert_eq!(loaded.settings, ohne_filter(), "{value}");
            assert_eq!(loaded.notice, None, "{value}");
        }
    }

    #[test]
    fn sprache_bleibt_gespeichert() {
        let path = alte_datei(&mit_filtern());
        let loaded = load(Some(&path));
        let app = AppSettings {
            language: Language::De,
            ..AppSettings::default()
        };
        save(&path, &loaded.settings, &loaded.kept, &app).unwrap();
        assert!(fs::read_to_string(&path).unwrap().contains(r#""language": "de""#));
        let again = load(Some(&path));
        assert_eq!(again.app, app);
        assert_eq!(again.settings, loaded.settings);
        assert_eq!(again.kept, loaded.kept);
    }

    #[test]
    fn zuruecksetzen_laesst_die_sprache_stehen() {
        let path = alte_datei(&mit_filtern());
        let loaded = load(Some(&path));
        let app = AppSettings {
            language: Language::De,
            ..AppSettings::default()
        };
        save(&path, &loaded.settings, &loaded.kept, &app).unwrap();
        let loaded = load(Some(&path));
        let (settings, kept) = reset_all(&loaded.settings, &loaded.kept).unwrap();
        save(&path, &settings, &kept, &loaded.app).unwrap();
        assert_eq!(load(Some(&path)).app.language, Language::De);
    }

    /// A file that could not be read back is never written.
    #[test]
    fn zu_grosse_datei_wird_nicht_gespeichert() {
        let path = temp_file("einstellungen.json");
        let small = Settings {
            split_tunnel: vec!["10.0.0.0/24".to_owned()],
            ..Settings::default()
        };
        save(&path, &small, &Filter::default(), &AppSettings::default()).unwrap();
        let networks = (0..4000u32)
            .map(|n| format!("10.{}.{}.0/24", n / 250, n % 250))
            .collect();
        let huge = Settings {
            split_tunnel: networks,
            ..Settings::default()
        };
        let error = save(&path, &huge, &Filter::default(), &AppSettings::default()).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        // The earlier file is still there and readable.
        let loaded = load(Some(&path));
        assert_eq!(loaded.notice, None);
        assert_eq!(loaded.settings, small);
    }

    /// Whatever `save` accepts, `load` reads back: just below the limit.
    #[test]
    fn was_gespeichert_wird_laesst_sich_laden() {
        let path = temp_file("einstellungen.json");
        let networks = (0..1500u32)
            .map(|n| format!("10.{}.{}.0/24", n / 250, n % 250))
            .collect();
        let many = Settings {
            split_tunnel: networks,
            ..Settings::default()
        };
        if save(&path, &many, &Filter::default(), &AppSettings::default()).is_ok() {
            assert_eq!(load(Some(&path)).settings, many);
        }
    }

    #[test]
    fn sprache_geht_nie_an_den_dienst() {
        let path = alte_datei(&ohne_filter());
        let app = AppSettings {
            language: Language::De,
            ..AppSettings::default()
        };
        save(&path, &ohne_filter(), &Filter::default(), &app).unwrap();
        let loaded = load(Some(&path));
        let body = to_service_json(&loaded.settings, "de.hideservers.net");
        assert!(!body.contains("language"), "{body}");
        assert!(!body.contains("\"de\""), "{body}");
    }

    // --- Favourites (feature 012) ---

    #[test]
    fn ohne_favoriten_ist_die_liste_leer() {
        assert!(app_aus("{}").app.favorites.is_empty());
        for value in ["null", "42", r#""de.hideservers.net""#, r#"{"a":1}"#, "true"] {
            let loaded = app_aus(&format!(
                r#"{{"donation_hint":false,"language":"de","favorites":{value}}}"#
            ));
            assert!(loaded.app.favorites.is_empty(), "{value}");
            assert!(!loaded.app.donation_hint, "{value}");
            assert_eq!(loaded.app.language, Language::De, "{value}");
            assert_eq!(loaded.settings, ohne_filter(), "{value}");
            assert_eq!(loaded.notice, None, "{value}");
        }
    }

    #[test]
    fn ungueltige_und_doppelte_favoriten_fallen_weg() {
        let loaded = app_aus(
            r#"{"language":"de","favorites":["b.hideservers.net", 7, null, "x.example.org", " A.HideServers.net ",
               "b.hideservers.net", "", ["c.hideservers.net"], "a.hideservers.net", "a b.hideservers.net"]}"#,
        );
        assert_eq!(loaded.app.favorites, ["b.hideservers.net", "a.hideservers.net"]);
        assert_eq!(loaded.app.language, Language::De);
        assert_eq!(loaded.notice, None);
    }

    #[test]
    fn hoechstens_256_favoriten_werden_gelesen() {
        let many: Vec<String> = (0..300).map(|n| format!("\"s{n}.hideservers.net\"")).collect();
        let loaded = app_aus(&format!(r#"{{"favorites":[{}]}}"#, many.join(",")));
        assert_eq!(loaded.app.favorites.len(), MAX_FAVORITES);
        assert_eq!(loaded.app.favorites[0], "s0.hideservers.net");
        assert_eq!(loaded.app.favorites[255], "s255.hideservers.net");
    }

    #[test]
    fn favoriten_bleiben_in_ihrer_reihenfolge_gespeichert() {
        let path = alte_datei(&mit_filtern());
        let loaded = load(Some(&path));
        let app = AppSettings {
            donation_hint: false,
            language: Language::De,
            favorites: vec![
                "streaming-jp.hideservers.net".to_owned(),
                "frankfurt1.hideservers.net".to_owned(),
                "fi.hideservers.net".to_owned(),
            ],
            ..AppSettings::default()
        };
        save(&path, &loaded.settings, &loaded.kept, &app).unwrap();
        let again = load(Some(&path));
        assert_eq!(again.app, app);
        assert_eq!(again.settings, loaded.settings);
        assert_eq!(again.kept, loaded.kept);
        assert_eq!(again.notice, None);
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("\"version\": 1"), "{content}");
        let mode = fs::metadata(&path).unwrap().permissions();
        assert_eq!(std::os::unix::fs::PermissionsExt::mode(&mode) & 0o777, 0o600);
    }

    #[test]
    fn favoriten_gehen_nie_an_den_dienst() {
        let path = alte_datei(&ohne_filter());
        let app = AppSettings {
            favorites: vec!["frankfurt1.hideservers.net".to_owned()],
            ..AppSettings::default()
        };
        save(&path, &ohne_filter(), &Filter::default(), &app).unwrap();
        let loaded = load(Some(&path));
        let body = to_service_json(&loaded.settings, "de.hideservers.net");
        assert!(!body.contains("frankfurt1"), "{body}");
        assert!(!body.contains("favorites"), "{body}");
    }

    // --- Stored appearance (feature 017) ---

    #[test]
    fn ohne_aussehen_gilt_hell() {
        assert_eq!(app_aus("{}").app.appearance, Appearance::Light);
        assert_eq!(app_aus(r#"{"donation_hint":false}"#).app.appearance, Appearance::Light);
        let path = temp_file("einstellungen.json");
        fs::write(&path, br#"{"version":1,"settings":{}}"#).unwrap();
        assert_eq!(load(Some(&path)).app.appearance, Appearance::Light);
        assert_eq!(load(Some(&temp_file("fehlt.json"))).app.appearance, Appearance::Light);
        assert_eq!(AppSettings::default().appearance, Appearance::Light);
    }

    #[test]
    fn dunkel_wird_gelesen() {
        for value in [r#""dark""#, r#"" DARK ""#, r#""Dark""#] {
            let loaded = app_aus(&format!(r#"{{"appearance":{value}}}"#));
            assert_eq!(loaded.app.appearance, Appearance::Dark, "{value}");
            assert!(loaded.app.appearance.is_dark(), "{value}");
        }
        assert_eq!(app_aus(r#"{"appearance":"light"}"#).app.appearance, Appearance::Light);
    }

    /// Anything but the text `dark` is light; the other settings stay as they are.
    #[test]
    fn unbekanntes_aussehen_gilt_als_hell_und_der_rest_bleibt() {
        let rest = r#""donation_hint":false,"language":"de","favorites":["berlin.hideservers.net"],
            "notifications":false,"keep_running":true,"update_check":true"#;
        let reference = app_aus(&format!("{{{rest}}}"));
        assert_eq!(reference.app.language, Language::De);
        assert!(reference.app.keep_running);
        for value in [
            "null",
            "42",
            "true",
            "[]",
            r#"{"a":1}"#,
            r#""Dunkel""#,
            r#""system""#,
            r#""""#,
            r#""darkish""#,
            r#""dar k""#,
        ] {
            let loaded = app_aus(&format!(r#"{{{rest},"appearance":{value}}}"#));
            assert_eq!(loaded.app.appearance, Appearance::Light, "{value}");
            assert_eq!(
                AppSettings {
                    appearance: Appearance::Light,
                    ..loaded.app.clone()
                },
                reference.app,
                "{value}"
            );
            assert_eq!(loaded.settings, reference.settings, "{value}");
            assert_eq!(loaded.notice, None, "{value}");
        }
    }

    #[test]
    fn aussehen_bleibt_gespeichert_und_der_rest_unberuehrt() {
        let path = alte_datei(&mit_filtern());
        let loaded = load(Some(&path));
        for appearance in [Appearance::Dark, Appearance::Light, Appearance::Dark] {
            let app = AppSettings {
                appearance,
                donation_hint: false,
                language: Language::De,
                favorites: vec!["berlin.hideservers.net".to_owned()],
                notifications: false,
                keep_running: true,
                update_check: true,
            };
            save(&path, &loaded.settings, &loaded.kept, &app).unwrap();
            let content = fs::read_to_string(&path).unwrap();
            assert!(
                content.contains(&format!(r#""appearance": "{}""#, appearance.code())),
                "{content}"
            );
            assert!(content.contains("\"version\": 1"), "{content}");
            let again = load(Some(&path));
            assert_eq!(again.app, app);
            assert_eq!(again.settings, loaded.settings);
            assert_eq!(again.kept, loaded.kept);
            assert_eq!(again.notice, None);
            let mode = fs::metadata(&path).unwrap().permissions();
            assert_eq!(std::os::unix::fs::PermissionsExt::mode(&mode) & 0o777, 0o600);
        }
    }

    /// A file of the version before this feature (no `appearance`) loads unchanged: light, everything else as it was.
    #[test]
    fn datei_ohne_aussehen_laedt_unveraendert() {
        let loaded = app_aus(r#"{"donation_hint":false,"language":"de","favorites":["berlin.hideservers.net"]}"#);
        assert_eq!(
            loaded.app,
            AppSettings {
                donation_hint: false,
                language: Language::De,
                favorites: vec!["berlin.hideservers.net".to_owned()],
                ..AppSettings::default()
            }
        );
        assert_eq!(loaded.app.appearance, Appearance::Light);
    }

    #[test]
    fn datei_mit_version_2_oder_kaputt_gibt_hell() {
        let path = temp_file("einstellungen.json");
        fs::write(&path, br#"{"version":2,"settings":{},"app":{"appearance":"dark"}}"#).unwrap();
        let loaded = load(Some(&path));
        assert_eq!(loaded.app.appearance, Appearance::Light);
        let path = temp_file("einstellungen.json");
        fs::write(&path, br#"{"version":1,"settings":{},"app":{"appearance":"dark""#).unwrap();
        assert_eq!(load(Some(&path)).app.appearance, Appearance::Light);
    }

    #[test]
    fn aussehen_geht_nie_an_den_dienst() {
        let path = alte_datei(&ohne_filter());
        let app = AppSettings {
            appearance: Appearance::Dark,
            ..AppSettings::default()
        };
        save(&path, &ohne_filter(), &Filter::default(), &app).unwrap();
        let loaded = load(Some(&path));
        assert_eq!(loaded.app.appearance, Appearance::Dark);
        let body = to_service_json(&loaded.settings, "de.hideservers.net");
        assert!(!body.contains("appearance") && !body.contains("dark"), "{body}");
        let body = to_service_json_without_host(&loaded.settings);
        assert!(!body.contains("appearance") && !body.contains("dark"), "{body}");
        // `changed_fields` compares only the service settings: the appearance is not part of them.
        assert!(changed_fields(&loaded.settings, &ohne_filter()).is_empty());
    }
}
