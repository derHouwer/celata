// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! Derives from state and lists what the user interface shows.
//!
//! The user interface (`ui/*.slint`) decides nothing that depends on the state. It receives the results
//! of these functions as finished values.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::daemon::ServiceState;
use crate::i18n::{self, Key, Language, Phrase};
use crate::names;
use crate::servers::{self, Country};
use crate::state::{DisplayState, StatusView};
use crate::streaming::{self, StreamingServer};

/// Colour of the status dot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Ok,
    Busy,
    Alert,
    Idle,
}

impl Tone {
    /// Value for the user interface.
    pub fn as_str(self) -> &'static str {
        match self {
            Tone::Ok => "ok",
            Tone::Busy => "busy",
            Tone::Alert => "alert",
            Tone::Idle => "idle",
        }
    }
}

/// Colour of the status dot for the display state. `None` means "Checking status …".
pub fn tone(display: Option<&DisplayState>) -> Tone {
    let Some(display) = display else { return Tone::Idle };
    // Deliberately without `_`: a new state is meant to cause a compile error here.
    match display {
        DisplayState::NotConnected => Tone::Idle,
        DisplayState::Connecting
        | DisplayState::Disconnecting
        | DisplayState::Switching { .. }
        | DisplayState::Reapplying { .. } => Tone::Busy,
        DisplayState::Connected { .. } => Tone::Ok,
        DisplayState::ConnectingBlocked
        | DisplayState::Blocked
        | DisplayState::ServiceUnreachableBlocked
        | DisplayState::ServiceUnreachable
        | DisplayState::NoPermission => Tone::Alert,
    }
}

/// What a click on the main button triggers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainAction {
    Connect,
    Disconnect,
    /// Not clickable.
    None,
}

impl MainAction {
    /// Value for the user interface.
    pub fn as_str(self) -> &'static str {
        match self {
            MainAction::Connect => "connect",
            MainAction::Disconnect => "disconnect",
            MainAction::None => "none",
        }
    }
}

/// What the right, red area of the main button offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SideAction {
    /// "Block internet", without connecting.
    Route,
    /// "Unblock internet".
    Unblock,
    /// No right area.
    None,
}

impl SideAction {
    /// Value for the user interface.
    pub fn as_str(self) -> &'static str {
        match self {
            SideAction::Route => "route",
            SideAction::Unblock => "unblock",
            SideAction::None => "none",
        }
    }

    /// Text of the whole button in the red state: from the moment the mouse enters the right area until
    /// it leaves the button, and for as long as the keyboard focus is on the right area. `None` exactly
    /// when the right area offers nothing.
    pub fn label(self) -> Option<Key> {
        match self {
            SideAction::Route => Some(Key::ButtonBlock),
            SideAction::Unblock => Some(Key::ButtonUnblock),
            SideAction::None => None,
        }
    }
}

/// Text and effect of the main button of the overview: main part and right area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MainButton {
    pub label: Key,
    pub action: MainAction,
    pub side: SideAction,
    /// `None` exactly when `side` offers nothing.
    pub side_label: Option<Key>,
}

/// Main button. The order of the checks matters: during a switch `can_disconnect` is true, yet the main
/// button stays disabled; "Disconnect" then exists as a small button of its own.
///
/// The right area offers "Unblock internet" as soon as that is possible (`can_unblock`, also during an
/// own connection attempt from the blocked state), otherwise "Block internet" in "Not connected" with
/// no operation running. Whether the kill switch is on in the settings is combined by the user interface
/// (`side-available`); `Monitor::begin_route` refuses otherwise.
pub fn main_button(display: Option<&DisplayState>, view: &StatusView) -> MainButton {
    let button = |label, action, side: SideAction| MainButton {
        label,
        action,
        side,
        side_label: side.label(),
    };
    let unblock = if view.can_unblock {
        SideAction::Unblock
    } else {
        SideAction::None
    };
    if matches!(display, Some(DisplayState::Reapplying { .. })) {
        button(Key::ButtonReconnecting, MainAction::None, SideAction::None)
    } else if view.switching {
        button(Key::ButtonSwitching, MainAction::None, SideAction::None)
    } else if view.connecting {
        button(Key::ButtonConnecting, MainAction::None, unblock)
    } else if display == Some(&DisplayState::Disconnecting) {
        button(Key::ButtonDisconnecting, MainAction::None, SideAction::None)
    } else if view.routing {
        button(Key::ButtonBlocking, MainAction::None, SideAction::None)
    } else if view.can_disconnect {
        button(Key::ButtonDisconnect, MainAction::Disconnect, SideAction::None)
    } else if view.can_connect {
        let side = if view.can_unblock {
            SideAction::Unblock
        } else if view.route_ready {
            SideAction::Route
        } else {
            SideAction::None
        };
        button(Key::ButtonConnect, MainAction::Connect, side)
    } else {
        button(Key::ButtonConnect, MainAction::None, unblock)
    }
}

/// Free locations only ("Free only" in the tab "Standard").
pub fn free_only(rows: Vec<LocationRow>) -> Vec<LocationRow> {
    rows.into_iter().filter(|row| row.free).collect()
}

/// A row of the location list, with texts in the language in force.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocationRow {
    /// Country code, key for the flag.
    pub code: String,
    /// Country name, bold.
    pub country: String,
    /// City, "Automatic, hide.me chooses the location" or "Streaming", small below it.
    pub place: String,
    /// Host for connecting.
    pub host: String,
    pub free: bool,
    /// Text for the selected location.
    pub choice: String,
}

fn choice(country: &Country, server: &servers::Server, language: Language) -> String {
    i18n::fill(
        i18n::text(language, Key::LocationChoice),
        &[country.name(language), server.name(language)],
    )
}

fn streaming_choice(server: &StreamingServer, language: Language) -> String {
    Phrase::fill(
        Key::LocationChoice,
        [Phrase::key(Key::StreamingPlace), server.country()],
    )
    .render(language)
}

/// One row per server, countries sorted by their name in `language`.
pub fn location_rows(countries: &[Country], language: Language) -> Vec<LocationRow> {
    let mut countries = countries.to_vec();
    servers::sort_by_name(&mut countries, language);
    countries
        .iter()
        .flat_map(|country| {
            country.servers.iter().map(move |server| LocationRow {
                code: country.code.clone(),
                country: country.name(language),
                place: if server.automatic {
                    i18n::text(language, Key::AutomaticPlace).to_owned()
                } else {
                    server.name(language)
                },
                host: server.host.clone(),
                free: server.free,
                choice: choice(country, server, language),
            })
        })
        .collect()
}

/// One row per streaming server, sorted by the country name in `language`.
pub fn streaming_rows(servers: &[StreamingServer], language: Language) -> Vec<LocationRow> {
    let mut rows: Vec<LocationRow> = servers
        .iter()
        .map(|server| LocationRow {
            code: server.code.clone(),
            country: server.name(language),
            place: i18n::text(language, Key::StreamingPlace).to_owned(),
            host: server.host.clone(),
            free: false,
            choice: streaming_choice(server, language),
        })
        .collect();
    rows.sort_by_key(|row| names::sort_key(&row.country));
    rows
}

/// Country code and text ("Germany, Frankfurt" or "Streaming, Japan") for a host, otherwise `None`.
pub fn location_of(host: &str, countries: &[Country], language: Language) -> Option<(String, String)> {
    let found = countries.iter().find_map(|country| {
        country
            .servers
            .iter()
            .find(|server| server.host.eq_ignore_ascii_case(host))
            .map(|server| (country.code.clone(), choice(country, server, language)))
    });
    found.or_else(|| {
        streaming::all()
            .into_iter()
            .find(|server| server.host.eq_ignore_ascii_case(host))
            .map(|server| (server.code.clone(), streaming_choice(&server, language)))
    })
}

/// Location that the overview shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverviewLocation {
    /// Country code for the flag, `None` for the placeholder.
    pub code: Option<String>,
    /// Empty means: the user interface shows "Choose location".
    pub label: String,
}

/// Location of the overview: the connected server takes precedence over the selected one.
pub fn overview_location(
    connected_host: &str,
    selected_host: &str,
    selected_name: &str,
    countries: &[Country],
    language: Language,
) -> OverviewLocation {
    if !connected_host.is_empty() {
        return match location_of(connected_host, countries, language) {
            Some((code, label)) => OverviewLocation {
                code: Some(code),
                label,
            },
            None => OverviewLocation {
                code: None,
                label: streaming::server_label(connected_host).render(language),
            },
        };
    }
    if !selected_host.is_empty() {
        // The selected name follows the language if the host is known; otherwise the text from the list
        // stays.
        return match location_of(selected_host, countries, language) {
            Some((code, label)) => OverviewLocation {
                code: Some(code),
                label,
            },
            None => OverviewLocation {
                code: None,
                label: selected_name.to_owned(),
            },
        };
    }
    OverviewLocation {
        code: None,
        label: String::new(),
    }
}

// ---------------------------------------------------------------------------------------------------
// Connection details and own IP address
// ---------------------------------------------------------------------------------------------------

/// Days since 1970-01-01 for a date (algorithm "days from civil" by Howard Hinnant).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_index = (month + 9) % 12;
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// Reads a time stamp such as `2026-11-09T12:34:56.123456789+01:00` or `…Z` (RFC 3339, as Go writes
/// it). Without new crates; fractions of seconds are skipped.
pub fn parse_rfc3339(text: &str) -> Option<SystemTime> {
    let bytes = text.as_bytes();
    if bytes.len() < 20 || bytes[4] != b'-' || bytes[7] != b'-' || !matches!(bytes[10], b'T' | b't') {
        return None;
    }
    if bytes[13] != b':' || bytes[16] != b':' {
        return None;
    }
    let number = |range: std::ops::Range<usize>| -> Option<i64> {
        let part = text.get(range)?;
        part.bytes()
            .all(|byte| byte.is_ascii_digit())
            .then(|| part.parse().ok())?
    };
    let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
    let (hour, minute, second) = (number(11..13)?, number(14..16)?, number(17..19)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let mut rest = &text[19..];
    if let Some(fraction) = rest.strip_prefix('.') {
        let digits = fraction.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            return None;
        }
        rest = &fraction[digits..];
    }
    let offset = match rest {
        "Z" | "z" => 0,
        _ => {
            let sign = match rest.as_bytes().first()? {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let zone = rest.get(1..)?;
            if zone.len() != 5 || zone.as_bytes()[2] != b':' {
                return None;
            }
            let hours: i64 = zone[0..2].parse().ok()?;
            let minutes: i64 = zone[3..5].parse().ok()?;
            sign * (hours * 3600 + minutes * 60)
        }
    };
    let seconds = days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second - offset;
    let seconds = u64::try_from(seconds).ok()?;
    UNIX_EPOCH.checked_add(Duration::from_secs(seconds))
}

/// Duration in the notation of the language: "1 h 4 min", "3 min", "less than 1 min" or the German
/// counterparts.
pub fn duration_text(duration: Duration, language: Language) -> String {
    let minutes = duration.as_secs() / 60;
    let number = |value: u64| value.to_string();
    match (minutes / 60, minutes % 60) {
        (0, 0) => i18n::text(language, Key::DurationUnderMinute).to_owned(),
        (0, minutes) => i18n::fill(i18n::text(language, Key::DurationMinutes), &[number(minutes)]),
        (hours, minutes) => i18n::fill(
            i18n::text(language, Key::DurationHoursMinutes),
            &[number(hours), number(minutes)],
        ),
    }
}

/// Amount of data in decimal units with the decimal separator of the language: "999 B", "1.5 kB",
/// "12.3 MB", "2.0 GB" or "1,5 kB".
pub fn bytes_text(bytes: u64, language: Language) -> String {
    const UNITS: [&str; 4] = ["kB", "MB", "GB", "TB"];
    if bytes < 1000 {
        return format!("{bytes} B");
    }
    // Calculate in tenths, so that nothing jumps over the unit through rounding.
    let mut tenths = bytes / 100;
    let mut unit = 0;
    while tenths >= 10_000 && unit + 1 < UNITS.len() {
        tenths /= 1000;
        unit += 1;
    }
    format!(
        "{}{}{} {}",
        tenths / 10,
        language.decimal_separator(),
        tenths % 10,
        UNITS[unit]
    )
}

/// Address without prefix for single addresses (`10.0.0.2/32` → `10.0.0.2`).
fn address_text(address: &str) -> &str {
    address
        .strip_suffix("/32")
        .or_else(|| address.strip_suffix("/128"))
        .unwrap_or(address)
}

/// Splits addresses into IPv4 and IPv6, each comma-separated; single addresses without prefix.
fn split_families(list: &[String]) -> (String, String) {
    let (mut v4, mut v6) = (Vec::new(), Vec::new());
    for item in list {
        let address = address_text(item);
        let ip = address.split('/').next().unwrap_or(address);
        match ip.parse::<std::net::IpAddr>() {
            Ok(std::net::IpAddr::V4(_)) => v4.push(address),
            Ok(std::net::IpAddr::V6(_)) => v6.push(address),
            Err(_) => {}
        }
    }
    (v4.join(", "), v6.join(", "))
}

/// Connection details for the overview; empty except in "Connected". Addresses separated into IPv4
/// and IPv6, so that the overview can show them in two columns.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConnectionDetails {
    pub duration: String,
    pub received: String,
    pub sent: String,
    pub address_v4: String,
    pub address_v6: String,
    pub dns_v4: String,
    pub dns_v6: String,
}

impl ConnectionDetails {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Details from the state of the service that was read last, only in "Connected".
pub fn connection_details(
    display: Option<&DisplayState>,
    state: Option<&ServiceState>,
    now: SystemTime,
    language: Language,
) -> ConnectionDetails {
    let (Some(DisplayState::Connected { .. }), Some(state)) = (display, state) else {
        return ConnectionDetails::default();
    };
    service_details(state, now, language)
}

/// Details from a state with code `connected`; otherwise empty.
pub fn service_details(state: &ServiceState, now: SystemTime, language: Language) -> ConnectionDetails {
    if state.code != "connected" {
        return ConnectionDetails::default();
    }
    let duration = state
        .since
        .as_deref()
        .and_then(parse_rfc3339)
        .map(|since| duration_text(now.duration_since(since).unwrap_or_default(), language))
        .unwrap_or_default();
    let (address_v4, address_v6) = split_families(&state.addresses);
    let (dns_v4, dns_v6) = split_families(&state.dns);
    ConnectionDetails {
        duration,
        received: state.rx.map(|bytes| bytes_text(bytes, language)).unwrap_or_default(),
        sent: state.tx.map(|bytes| bytes_text(bytes, language)).unwrap_or_default(),
        address_v4,
        address_v6,
        dns_v4,
        dns_v6,
    }
}

/// How long Celata waits for the public address at most.
pub const EXTERNAL_IP_TIMEOUT: Duration = Duration::from_secs(10);
/// After a failure Celata asks again only after this time.
pub const EXTERNAL_IP_RETRY: Duration = Duration::from_secs(30);

/// State of the public address. Celata queries it itself as soon as "Connected" or "Not connected" is
/// reached (instead of a button). Never in "Internet blocked": the query would need exception routes
/// that bypass the block; the overview then shows "Internet blocked" in red.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ExternalIp {
    #[default]
    Idle,
    Asking {
        since: Instant,
    },
    Known {
        v4: Option<String>,
        v6: Option<String>,
    },
    Failed {
        at: Instant,
    },
}

impl ExternalIp {
    /// Accepts the report `external ips`, but only while a query is running.
    pub fn answer(&mut self, v4: Option<&str>, v6: Option<&str>, now: Instant) {
        if !matches!(self, Self::Asking { .. }) {
            return;
        }
        let clean = |ip: Option<&str>| {
            ip.map(str::trim)
                .filter(|ip| ip.parse::<std::net::IpAddr>().is_ok())
                .map(str::to_owned)
        };
        let (v4, v6) = (clean(v4), clean(v6));
        *self = if v4.is_none() && v6.is_none() {
            Self::Failed { at: now }
        } else {
            Self::Known { v4, v6 }
        };
    }

    /// After 10 seconds without a report the query counts as failed.
    pub fn expire(&mut self, now: Instant) -> bool {
        if let Self::Asking { since } = self
            && now.saturating_duration_since(*since) >= EXTERNAL_IP_TIMEOUT
        {
            *self = Self::Failed { at: now };
            return true;
        }
        false
    }

    /// Should it be queried (again) now? Never queried yet, or 30 seconds after a failure.
    pub fn due(&self, now: Instant) -> bool {
        match self {
            Self::Idle => true,
            Self::Failed { at } => now.saturating_duration_since(*at) >= EXTERNAL_IP_RETRY,
            Self::Asking { .. } | Self::Known { .. } => false,
        }
    }

    /// Display in the column IPv4 or IPv6: the address, "–" without an address of this kind, otherwise the state.
    pub fn column(&self, v6: bool, language: Language) -> String {
        match self {
            Self::Idle => String::new(),
            Self::Asking { .. } => i18n::text(language, Key::ExternalIpAsking).to_owned(),
            Self::Failed { .. } => i18n::text(language, Key::ExternalIpFailed).to_owned(),
            Self::Known { v4: four, v6: six } => {
                let ip = if v6 { six } else { four };
                ip.clone().unwrap_or_else(|| "–".to_owned())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::{Error, Observation, ServiceState};
    use crate::servers::{self, parse_list};
    use crate::state::Monitor;

    const FIXTURE: &str = include_str!("../tests/fixtures/servers.json");

    fn fixture() -> Vec<Country> {
        parse_list(FIXTURE.as_bytes()).unwrap()
    }

    // --- Tone ---

    #[test]
    fn punktfarbe_je_zustand() {
        let cases = [
            (DisplayState::NotConnected, Tone::Idle),
            (DisplayState::Connecting, Tone::Busy),
            (DisplayState::Disconnecting, Tone::Busy),
            (
                DisplayState::Switching {
                    target: "de.hideservers.net".to_owned(),
                    back: false,
                },
                Tone::Busy,
            ),
            (
                DisplayState::Switching {
                    target: "nl.hideservers.net".to_owned(),
                    back: true,
                },
                Tone::Busy,
            ),
            (DisplayState::Reapplying { back: false }, Tone::Busy),
            (DisplayState::Reapplying { back: true }, Tone::Busy),
            (
                DisplayState::Connected {
                    server: Some("nl.hideservers.net".to_owned()),
                },
                Tone::Ok,
            ),
            (DisplayState::Connected { server: None }, Tone::Ok),
            (DisplayState::ConnectingBlocked, Tone::Alert),
            (DisplayState::Blocked, Tone::Alert),
            (DisplayState::ServiceUnreachableBlocked, Tone::Alert),
            (DisplayState::ServiceUnreachable, Tone::Alert),
            (DisplayState::NoPermission, Tone::Alert),
        ];
        for (display, expected) in cases {
            assert_eq!(tone(Some(&display)), expected, "{display:?}");
        }
    }

    #[test]
    fn punktfarbe_vor_der_ersten_beobachtung() {
        assert_eq!(tone(None), Tone::Idle);
    }

    #[test]
    fn punktfarbe_als_text() {
        assert_eq!(Tone::Ok.as_str(), "ok");
        assert_eq!(Tone::Busy.as_str(), "busy");
        assert_eq!(Tone::Alert.as_str(), "alert");
        assert_eq!(Tone::Idle.as_str(), "idle");
    }

    // --- MainButton ---

    fn state(monitor: &mut Monitor, code: &str, host: Option<&str>, blocked: bool) {
        let state = ServiceState {
            code: code.to_owned(),
            host: host.map(str::to_owned),
            ..ServiceState::default()
        };
        monitor.observe(Observation::State { state, blocked });
    }

    fn button(monitor: &Monitor) -> (&'static str, MainAction) {
        let result = main_button(monitor.display(), &monitor.view());
        (i18n::text(Language::De, result.label), result.action)
    }

    fn connected() -> Monitor {
        let mut monitor = Monitor::new();
        state(&mut monitor, "connected", Some("nl.hideservers.net"), true);
        monitor
    }

    #[test]
    fn hauptknopf_getrennt() {
        let mut monitor = Monitor::new();
        state(&mut monitor, "clean", None, false);
        assert_eq!(button(&monitor), ("Verbinde VPN", MainAction::Connect));
    }

    #[test]
    fn hauptknopf_beim_verbinden() {
        let mut monitor = Monitor::new();
        state(&mut monitor, "clean", None, false);
        monitor.begin_connect("de.hideservers.net").unwrap();
        assert_eq!(button(&monitor), ("Wird verbunden …", MainAction::None));
    }

    #[test]
    fn hauptknopf_verbunden() {
        assert_eq!(button(&connected()), ("Trennen", MainAction::Disconnect));
    }

    #[test]
    fn hauptknopf_beim_wechsel_gesperrt_obwohl_trennen_moeglich() {
        let mut monitor = connected();
        monitor.begin_connect("de.hideservers.net").unwrap();
        assert!(monitor.view().can_disconnect);
        assert_eq!(button(&monitor), ("Wird gewechselt …", MainAction::None));
    }

    #[test]
    fn hauptknopf_beim_trennen() {
        let mut monitor = connected();
        monitor.begin_disconnect(false);
        assert_eq!(monitor.display(), Some(&DisplayState::Disconnecting));
        assert_eq!(button(&monitor), ("Wird getrennt …", MainAction::None));
    }

    #[test]
    fn hauptknopf_bei_gesperrtem_internet() {
        let mut monitor = Monitor::new();
        state(&mut monitor, "routed", None, true);
        assert_eq!(monitor.display(), Some(&DisplayState::Blocked));
        assert_eq!(button(&monitor), ("Verbinde VPN", MainAction::Connect));
    }

    #[test]
    fn hauptknopf_ohne_dienst() {
        let mut monitor = Monitor::new();
        monitor.observe(Observation::Failed {
            error: Error::NotRunning,
            blocked: false,
        });
        assert_eq!(monitor.display(), Some(&DisplayState::ServiceUnreachable));
        assert_eq!(button(&monitor), ("Verbinde VPN", MainAction::None));
    }

    #[test]
    fn hauptknopf_ohne_berechtigung() {
        let mut monitor = Monitor::new();
        monitor.observe(Observation::Failed {
            error: Error::PermissionDenied,
            blocked: false,
        });
        assert_eq!(monitor.display(), Some(&DisplayState::NoPermission));
        assert_eq!(button(&monitor), ("Verbinde VPN", MainAction::None));
    }

    #[test]
    fn hauptknopf_vor_der_ersten_beobachtung() {
        assert_eq!(button(&Monitor::new()), ("Verbinde VPN", MainAction::None));
    }

    #[test]
    fn wirkung_als_text() {
        assert_eq!(MainAction::Connect.as_str(), "connect");
        assert_eq!(MainAction::Disconnect.as_str(), "disconnect");
        assert_eq!(MainAction::None.as_str(), "none");
    }

    // --- Right area of the main button ---

    /// Right area; checks in passing: the text is empty exactly when nothing is offered.
    fn side(monitor: &Monitor) -> (SideAction, &'static str) {
        let result = main_button(monitor.display(), &monitor.view());
        assert_eq!(
            result.side_label.is_none(),
            result.side == SideAction::None,
            "{result:?}"
        );
        (
            result.side,
            result.side_label.map_or("", |label| i18n::text(Language::De, label)),
        )
    }

    #[test]
    fn rechter_bereich_bietet_sperren_bei_nicht_verbunden() {
        let mut monitor = Monitor::new();
        state(&mut monitor, "clean", None, false);
        assert_eq!(side(&monitor), (SideAction::Route, "Internet sperren"));
        assert_eq!(button(&monitor), ("Verbinde VPN", MainAction::Connect));
    }

    #[test]
    fn rechter_bereich_fehlt_ohne_zustand_bei_verbindung_und_ohne_dienst() {
        let nothing = (SideAction::None, "");
        assert_eq!(side(&Monitor::new()), nothing);
        assert_eq!(side(&connected()), nothing);
        for (error, blocked) in [
            (Error::NotRunning, false),
            (Error::NotRunning, true),
            (Error::PermissionDenied, false),
        ] {
            let mut monitor = Monitor::new();
            monitor.observe(Observation::Failed { error, blocked });
            assert_eq!(side(&monitor), nothing, "{:?}", monitor.display());
        }
    }

    #[test]
    fn rechter_bereich_fehlt_waehrend_eines_vorgangs_ohne_sperre() {
        let nothing = (SideAction::None, "");
        // Connecting from "Not connected"
        let mut connecting = Monitor::new();
        state(&mut connecting, "clean", None, false);
        connecting.begin_connect("de.hideservers.net").unwrap();
        assert_eq!(side(&connecting), nothing);
        // Switching
        let mut switching = connected();
        switching.begin_connect("de.hideservers.net").unwrap();
        assert!(switching.view().switching);
        assert_eq!(side(&switching), nothing);
        // Disconnecting
        let mut disconnecting = connected();
        disconnecting.begin_disconnect(false);
        assert_eq!(side(&disconnecting), nothing);
        // Applying settings
        let mut reapplying = connected();
        reapplying.begin_reapply().unwrap();
        assert!(matches!(reapplying.display(), Some(DisplayState::Reapplying { .. })));
        assert_eq!(side(&reapplying), nothing);
    }

    #[test]
    fn hauptknopf_waehrend_des_sperrens() {
        let mut monitor = Monitor::new();
        state(&mut monitor, "clean", None, false);
        assert!(monitor.begin_route(true));
        assert_eq!(button(&monitor), ("Internet wird gesperrt …", MainAction::None));
        assert_eq!(side(&monitor), (SideAction::None, ""));
    }

    #[test]
    fn rechter_bereich_bietet_aufheben_bei_gesperrtem_internet() {
        let mut monitor = Monitor::new();
        state(&mut monitor, "routed", None, true);
        assert_eq!(monitor.display(), Some(&DisplayState::Blocked));
        assert_eq!(button(&monitor), ("Verbinde VPN", MainAction::Connect));
        assert_eq!(side(&monitor), (SideAction::Unblock, "Sperre aufheben"));
    }

    #[test]
    fn rechter_bereich_bietet_aufheben_wenn_der_dienst_selbst_neu_verbindet() {
        let mut monitor = Monitor::new();
        state(&mut monitor, "connecting", None, true);
        assert_eq!(monitor.display(), Some(&DisplayState::ConnectingBlocked));
        // The main part is not clickable, the way out remains anyway.
        assert_eq!(button(&monitor), ("Verbinde VPN", MainAction::None));
        assert_eq!(side(&monitor), (SideAction::Unblock, "Sperre aufheben"));
    }

    #[test]
    fn rechter_bereich_beim_eigenen_aufbau_aus_der_sperre() {
        let mut monitor = Monitor::new();
        state(&mut monitor, "routed", None, true);
        monitor.begin_connect("de.hideservers.net").unwrap();
        assert_eq!(button(&monitor), ("Wird verbunden …", MainAction::None));
        // The service reports the connection attempt; the block is still in place.
        state(&mut monitor, "connecting", None, true);
        assert!(
            monitor.view().can_unblock,
            "auch während des eigenen Aufbaus gibt es einen Ausweg"
        );
        assert_eq!(button(&monitor), ("Wird verbunden …", MainAction::None));
        assert_eq!(side(&monitor), (SideAction::Unblock, "Sperre aufheben"));
    }

    #[test]
    fn rechter_bereich_fehlt_bei_sperre_ohne_dienst() {
        let mut monitor = Monitor::new();
        monitor.observe(Observation::Failed {
            error: Error::NotRunning,
            blocked: true,
        });
        assert_eq!(monitor.display(), Some(&DisplayState::ServiceUnreachableBlocked));
        assert_eq!(side(&monitor), (SideAction::None, ""));
    }

    #[test]
    fn aufheben_hat_vorrang_und_folgt_genau_can_unblock() {
        let mut cases = Vec::new();
        for (code, blocked) in [
            ("clean", false),
            ("routed", true),
            ("dpd timeout", true),
            ("connecting", true),
            ("connecting", false),
            ("connected", true),
        ] {
            let mut monitor = Monitor::new();
            state(&mut monitor, code, Some("nl.hideservers.net"), blocked);
            cases.push(monitor);
        }
        let mut own = Monitor::new();
        state(&mut own, "routed", None, true);
        own.begin_connect("de.hideservers.net").unwrap();
        cases.push(own);
        let mut lifting = Monitor::new();
        state(&mut lifting, "routed", None, true);
        assert!(lifting.ask_unblock() && lifting.confirm_unblock());
        cases.push(lifting);
        for monitor in &cases {
            let (action, _) = side(monitor);
            assert_eq!(
                action == SideAction::Unblock,
                monitor.view().can_unblock,
                "{:?}",
                monitor.display()
            );
            if monitor.view().can_unblock {
                assert_ne!(action, SideAction::Route, "{:?}", monitor.display());
            }
        }
    }

    #[test]
    fn rechter_bereich_als_text() {
        assert_eq!(SideAction::Route.as_str(), "route");
        assert_eq!(SideAction::Unblock.as_str(), "unblock");
        assert_eq!(SideAction::None.as_str(), "none");
    }

    // --- location_of and OverviewLocation ---

    #[test]
    fn standort_eines_bekannten_servers() {
        let countries = fixture();
        assert_eq!(
            location_of("frankfurt.hideservers.net", &countries, Language::De),
            Some(("DE".to_owned(), "Deutschland, Frankfurt".to_owned()))
        );
    }

    #[test]
    fn standort_eines_streaming_servers() {
        assert_eq!(
            location_of("streaming-uk.hideservers.net", &fixture(), Language::De),
            Some(("GB".to_owned(), "Streaming, Vereinigtes Königreich".to_owned()))
        );
    }

    #[test]
    fn standort_eines_unbekannten_servers() {
        assert_eq!(location_of("unbekannt.hideservers.net", &fixture(), Language::De), None);
        assert_eq!(location_of("frankfurt.hideservers.net", &[], Language::De), None);
    }

    #[test]
    fn uebersicht_zeigt_den_verbundenen_bekannten_server() {
        let location = overview_location("frankfurt.hideservers.net", "", "", &fixture(), Language::De);
        assert_eq!(
            location,
            OverviewLocation {
                code: Some("DE".to_owned()),
                label: "Deutschland, Frankfurt".to_owned()
            }
        );
    }

    #[test]
    fn uebersicht_zeigt_den_verbundenen_unbekannten_server() {
        let location = overview_location("unbekannt.hideservers.net", "", "", &fixture(), Language::De);
        assert_eq!(
            location,
            OverviewLocation {
                code: None,
                label: "unbekannt.hideservers.net".to_owned()
            }
        );
    }

    #[test]
    fn uebersicht_zeigt_den_gewaehlten_server() {
        let countries = fixture();
        let location = overview_location(
            "",
            "au.hideservers.net",
            "Australien, Automatisch",
            &countries,
            Language::De,
        );
        assert_eq!(
            location,
            OverviewLocation {
                code: Some("AU".to_owned()),
                label: "Australien, Automatisch".to_owned()
            }
        );
        let unknown = overview_location("", "unbekannt.hideservers.net", "Irgendwo", &countries, Language::De);
        assert_eq!(
            unknown,
            OverviewLocation {
                code: None,
                label: "Irgendwo".to_owned()
            }
        );
    }

    #[test]
    fn uebersicht_ohne_wahl() {
        assert_eq!(
            overview_location("", "", "", &fixture(), Language::De),
            OverviewLocation {
                code: None,
                label: String::new()
            }
        );
    }

    #[test]
    fn verbundener_server_hat_vorrang_vor_dem_gewaehlten() {
        let location = overview_location(
            "streaming-uk.hideservers.net",
            "au.hideservers.net",
            "Australien, Automatisch",
            &fixture(),
            Language::De,
        );
        assert_eq!(location.code.as_deref(), Some("GB"));
        assert_eq!(location.label, "Streaming, Vereinigtes Königreich");
    }

    // --- LocationRow ---

    #[test]
    fn zeilen_nach_laendern_der_sprache_sortiert() {
        // The list comes unsorted from the API; it is sorted by the language in force.
        let countries = fixture();
        let order = |language| {
            let mut seen: Vec<String> = Vec::new();
            for row in location_rows(&servers::search(&countries, ""), language) {
                if seen.last() != Some(&row.country) {
                    seen.push(row.country);
                }
            }
            seen
        };
        assert_eq!(
            order(Language::De),
            ["Australien", "Deutschland", "Neuseeland", "Niederlande"]
        );
        assert_eq!(
            order(Language::En),
            ["Australia", "Germany", "Netherlands", "New Zealand"]
        );
        // Within a country the order of the servers stays.
        let rows = location_rows(&countries, Language::De);
        let germany: Vec<&str> = rows
            .iter()
            .filter(|row| row.code == "DE")
            .map(|row| row.host.as_str())
            .collect();
        assert_eq!(
            germany,
            [
                "de.hideservers.net",
                "frankfurt.hideservers.net",
                "berlin.hideservers.net",
                "free-de.hideservers.net"
            ]
        );
    }

    #[test]
    fn zeilen_auf_englisch() {
        let rows = location_rows(&fixture(), Language::En);
        assert_eq!(rows[0].country, "Australia");
        assert_eq!(rows[0].place, "Automatic, hide.me chooses the location");
        assert_eq!(rows[0].choice, "Australia, Automatic");
        let streaming = streaming_rows(&streaming::all(), Language::En);
        let uk = streaming.iter().find(|row| row.code == "GB").unwrap();
        assert_eq!(uk.country, "United Kingdom");
        assert_eq!(uk.choice, "Streaming, United Kingdom");
        // Sorted in English: "Austria" before "Brazil", "Germany" before "Italy".
        let names: Vec<&str> = streaming.iter().map(|row| row.country.as_str()).collect();
        assert!(
            names
                .windows(2)
                .all(|pair| crate::names::sort_key(pair[0]) <= crate::names::sort_key(pair[1])),
            "{names:?}"
        );
        assert_eq!(
            location_of("frankfurt.hideservers.net", &fixture(), Language::En),
            Some(("DE".to_owned(), "Germany, Frankfurt".to_owned()))
        );
    }

    #[test]
    fn dauer_und_datenmenge_auf_englisch() {
        assert_eq!(duration_text(Duration::from_secs(59), Language::En), "less than 1 min");
        assert_eq!(duration_text(Duration::from_secs(180), Language::En), "3 min");
        assert_eq!(duration_text(Duration::from_mins(64), Language::En), "1 h 4 min");
        assert_eq!(bytes_text(999, Language::En), "999 B");
        assert_eq!(bytes_text(1500, Language::En), "1.5 kB");
        assert_eq!(bytes_text(12_345_678, Language::En), "12.3 MB");
        assert_eq!(bytes_text(2_000_000_000, Language::En), "2.0 GB");
    }

    #[test]
    fn hauptknopf_auf_englisch() {
        let mut monitor = Monitor::new();
        state(&mut monitor, "clean", None, false);
        let result = main_button(monitor.display(), &monitor.view());
        assert_eq!(i18n::text(Language::En, result.label), "Connect VPN");
        assert_eq!(
            result.side_label.map(|label| i18n::text(Language::En, label)),
            Some("Block internet")
        );
    }

    #[test]
    fn automatisch_steht_zuerst_mit_erklaerung() {
        let rows = location_rows(&fixture(), Language::De);
        assert_eq!(rows[0].code, "AU");
        assert_eq!(rows[0].place, "Automatisch, hide.me wählt den Ort");
        assert_eq!(rows[0].choice, "Australien, Automatisch");
        assert_eq!(rows[1].place, "Melbourne");
        assert_eq!(rows[1].choice, "Australien, Melbourne");
    }

    #[test]
    fn zeilen_behalten_auswahltext_und_kostenlos() {
        let rows = location_rows(&fixture(), Language::De);
        let frankfurt = rows.iter().find(|row| row.host == "frankfurt.hideservers.net").unwrap();
        assert_eq!(frankfurt.choice, "Deutschland, Frankfurt");
        assert!(!frankfurt.free);
        assert!(rows.iter().any(|row| row.code == "DE" && row.free));
    }

    #[test]
    fn suche_wirkt_auf_die_zeilen() {
        let rows = location_rows(&servers::search(&fixture(), "perth"), Language::De);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].choice, "Australien, Perth");
    }

    #[test]
    fn streaming_zeile() {
        let rows = streaming_rows(&streaming::all(), Language::De);
        let uk = rows.iter().find(|row| row.code == "GB").unwrap();
        assert_eq!(
            uk,
            &LocationRow {
                code: "GB".to_owned(),
                country: "Vereinigtes Königreich".to_owned(),
                place: "Streaming".to_owned(),
                host: "streaming-uk.hideservers.net".to_owned(),
                free: false,
                choice: "Streaming, Vereinigtes Königreich".to_owned(),
            }
        );
    }

    // --- Connection details and own IP address ---

    fn at(seconds: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(seconds)
    }

    #[test]
    fn zeitstempel_wie_go_sie_schreibt() {
        assert_eq!(parse_rfc3339("1970-01-01T00:00:00Z"), Some(at(0)));
        assert_eq!(parse_rfc3339("2026-11-09T12:34:56Z"), Some(at(1_794_227_696)));
        assert_eq!(
            parse_rfc3339("2026-11-09T13:34:56.123456789+01:00"),
            Some(at(1_794_227_696))
        );
        assert_eq!(parse_rfc3339("2026-11-09T11:34:56-01:00"), Some(at(1_794_227_696)));
        assert_eq!(parse_rfc3339("2024-02-29T00:00:00Z"), Some(at(1_709_164_800)));
        for bad in [
            "",
            "2026-11-09",
            "2026-13-01T00:00:00Z",
            "2026-11-09T12:34:56",
            "2026-11-09T12:34:56.Z",
            "2026-11-09T12:34:56+0100",
            "x026-11-09T12:34:56Z",
        ] {
            assert_eq!(parse_rfc3339(bad), None, "{bad}");
        }
    }

    #[test]
    fn dauer_und_datenmenge_auf_deutsch() {
        assert_eq!(duration_text(Duration::from_secs(59), Language::De), "unter 1 Min.");
        assert_eq!(duration_text(Duration::from_secs(180), Language::De), "3 Min.");
        assert_eq!(duration_text(Duration::from_mins(64), Language::De), "1 Std. 4 Min.");
        assert_eq!(bytes_text(999, Language::De), "999 B");
        assert_eq!(bytes_text(1500, Language::De), "1,5 kB");
        assert_eq!(bytes_text(12_345_678, Language::De), "12,3 MB");
        assert_eq!(bytes_text(2_000_000_000, Language::De), "2,0 GB");
        assert_eq!(bytes_text(999_999, Language::De), "999,9 kB");
    }

    fn connected_details() -> ServiceState {
        ServiceState {
            code: "connected".to_owned(),
            host: Some("nl.hideservers.net".to_owned()),
            rx: Some(12_345_678),
            tx: Some(2_345_678),
            since: Some("2026-11-09T12:00:00Z".to_owned()),
            addresses: vec!["10.128.0.2/32".to_owned(), "fd00::2/128".to_owned()],
            dns: vec!["10.128.0.1".to_owned()],
            ..ServiceState::default()
        }
    }

    #[test]
    fn details_nur_bei_verbunden() {
        let state = connected_details();
        let connected = DisplayState::Connected {
            server: state.host.clone(),
        };
        let now = parse_rfc3339("2026-11-09T13:04:30Z").unwrap();
        let details = connection_details(Some(&connected), Some(&state), now, Language::De);
        assert_eq!(
            details,
            ConnectionDetails {
                duration: "1 Std. 4 Min.".to_owned(),
                received: "12,3 MB".to_owned(),
                sent: "2,3 MB".to_owned(),
                address_v4: "10.128.0.2".to_owned(),
                address_v6: "fd00::2".to_owned(),
                dns_v4: "10.128.0.1".to_owned(),
                dns_v6: String::new(),
            }
        );
        for display in [
            DisplayState::NotConnected,
            DisplayState::Blocked,
            DisplayState::Reapplying { back: false },
        ] {
            assert!(connection_details(Some(&display), Some(&state), now, Language::De).is_empty());
        }
        assert!(connection_details(Some(&connected), None, now, Language::De).is_empty());
    }

    #[test]
    fn oeffentliche_adresse() {
        let start = Instant::now();
        let mut ip = ExternalIp::Idle;
        assert!(ip.due(start));
        // Without a query no report is accepted.
        ip.answer(Some("203.0.113.7"), None, start);
        assert_eq!(ip, ExternalIp::Idle);
        ip = ExternalIp::Asking { since: start };
        assert!(!ip.due(start));
        assert_eq!(ip.column(false, Language::De), "wird ermittelt …");
        assert!(!ip.expire(start + Duration::from_secs(9)));
        ip.answer(Some("203.0.113.7"), None, start);
        assert_eq!(
            (ip.column(false, Language::De), ip.column(true, Language::De)),
            ("203.0.113.7".to_owned(), "–".to_owned())
        );
        assert!(!ip.due(start + Duration::from_secs(3600)));
        let mut empty = ExternalIp::Asking { since: start };
        empty.answer(Some(""), Some("kaputt"), start);
        assert_eq!(empty, ExternalIp::Failed { at: start });
        assert_eq!(empty.column(true, Language::De), "nicht ermittelbar");
        assert!(!empty.due(start + Duration::from_secs(29)));
        assert!(empty.due(start + Duration::from_secs(30)));
        let mut late = ExternalIp::Asking { since: start };
        assert!(late.expire(start + Duration::from_secs(10)));
        assert!(matches!(late, ExternalIp::Failed { .. }));
    }

    // --- Free only ---

    #[test]
    fn nur_kostenlose_zeilen() {
        let rows = location_rows(&fixture(), Language::De);
        assert!(rows.len() > 1);
        let free = free_only(rows);
        assert_eq!(
            free.iter().map(|row| row.host.as_str()).collect::<Vec<_>>(),
            ["free-de.hideservers.net"]
        );
        assert!(free.iter().all(|row| row.free));
    }
}
