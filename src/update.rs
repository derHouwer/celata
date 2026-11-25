// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! Hint about a new version: off by default, at most once at start and once a day, never past a block of the
//! internet. The only request is the one in [`fetch_latest`]; it sends nothing besides the request itself, and
//! from the answer only three numbers are used. Nothing is downloaded or installed.

use std::fmt;
use std::io;
use std::time::{Duration, Instant};

use crate::cli;
use crate::net;
use crate::state::DisplayState;

/// Page with the releases of Celata, opened in the browser.
pub const RELEASES_URL: &str = "https://github.com/derHouwer/celata/releases";
/// Newest release; GitHub never answers with drafts or pre-releases here.
pub const LATEST_API: &str = "https://api.github.com/repos/derHouwer/celata/releases/latest";
/// Largest answer that is read at all.
pub const MAX_ANSWER: u64 = 256 * 1024;
/// Time between two checks.
pub const INTERVAL: Duration = Duration::from_hours(24);
/// Most digits of one part of a version.
const MAX_DIGITS: usize = 6;

/// A version as three numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u32, pub u32, pub u32);

impl fmt::Display for Version {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}.{}", self.0, self.1, self.2)
    }
}

/// Reads `v1.2.3` or `1.2.3`; anything else (a suffix such as `-rc1`, two or four parts, long numbers) is
/// no version.
pub fn parse_tag(tag: &str) -> Option<Version> {
    let text = tag.strip_prefix('v').unwrap_or(tag);
    let mut parts = text.split('.');
    let mut next = || {
        let part = parts.next()?;
        if part.is_empty() || part.len() > MAX_DIGITS || !part.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        part.parse().ok()
    };
    let version = Version(next()?, next()?, next()?);
    parts.next().is_none().then_some(version)
}

/// Version of this Celata.
pub fn own() -> Option<Version> {
    parse_tag(cli::VERSION)
}

/// The version in the answer of GitHub if it is newer than `own`. Drafts, pre-releases, broken answers and
/// answers larger than [`MAX_ANSWER`] give nothing.
pub fn newer(answer: &[u8], own: Version) -> Option<Version> {
    if u64::try_from(answer.len()).map_or(true, |length| length > MAX_ANSWER) {
        return None;
    }
    let value: serde_json::Value = serde_json::from_slice(answer).ok()?;
    let flag = |name: &str| value.get(name).and_then(serde_json::Value::as_bool).unwrap_or(false);
    if flag("draft") || flag("prerelease") {
        return None;
    }
    let version = parse_tag(value.get("tag_name")?.as_str()?)?;
    (version > own).then_some(version)
}

/// When the next check is due, and which version was shown in this session. Only in memory.
#[derive(Debug, Default)]
pub struct Schedule {
    next_due: Option<Instant>,
    shown: Option<Version>,
}

impl Schedule {
    /// Due at the start and after switching the check on.
    pub fn due(&self, now: Instant) -> bool {
        self.next_due.is_none_or(|due| now >= due)
    }

    /// A due check took place, failed or was left out because of a block: the next one in 24 hours.
    pub fn done(&mut self, now: Instant) {
        self.next_due = Some(now + INTERVAL);
    }

    /// The switch was turned on: due at once.
    pub fn enable(&mut self) {
        self.next_due = None;
    }

    /// True only for a version larger than the one shown so far.
    pub fn record(&mut self, version: Version) -> bool {
        if self.shown.is_some_and(|shown| shown >= version) {
            return false;
        }
        self.shown = Some(version);
        true
    }

    pub fn shown(&self) -> Option<Version> {
        self.shown
    }
}

/// Never past a block: not while the internet is blocked as shown, and not if the routes block it or cannot be
/// read.
pub fn may_check(display: Option<&DisplayState>, routes: &io::Result<bool>) -> bool {
    !matches!(
        display,
        Some(DisplayState::Blocked | DisplayState::ConnectingBlocked | DisplayState::ServiceUnreachableBlocked)
    ) && matches!(routes, Ok(false))
}

/// Asks GitHub for the newest release, the only request of this module. `routes` is read once more right
/// before; if it blocks or cannot be read, nothing is sent.
pub fn fetch_latest(routes: impl Fn() -> io::Result<bool>) -> Option<Version> {
    if !matches!(routes(), Ok(false)) {
        return None;
    }
    let answer = net::get(LATEST_API, MAX_ANSWER).ok()?;
    newer(&answer, own()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versionsnummern_lesen() {
        assert_eq!(parse_tag("v0.9.0"), Some(Version(0, 9, 0)));
        assert_eq!(parse_tag("0.9.0"), Some(Version(0, 9, 0)));
        assert_eq!(parse_tag("v12.345.678901"), Some(Version(12, 345, 678_901)));
        for tag in [
            "v0.9.0-rc1",
            "0.9",
            "v1.2.3.4",
            "v01234567.0.0",
            "",
            "vx.y.z",
            "v",
            "1..2",
            " 1.2.3",
            "V1.2.3",
            "+1.2.3",
        ] {
            assert_eq!(parse_tag(tag), None, "{tag:?}");
        }
        assert_eq!(own(), parse_tag(cli::VERSION));
        assert!(own().is_some());
        assert_eq!(Version(0, 9, 0).to_string(), "0.9.0");
    }

    fn answer(tag: &str, draft: bool, prerelease: bool) -> Vec<u8> {
        serde_json::json!({"tag_name": tag, "draft": draft, "prerelease": prerelease, "html_url": "https://evil.example"})
            .to_string()
            .into_bytes()
    }

    #[test]
    fn nur_neuere_veroeffentlichte_versionen_zaehlen() {
        let own = Version(0, 8, 1);
        assert_eq!(newer(&answer("v0.9.0", false, false), own), Some(Version(0, 9, 0)));
        assert_eq!(newer(&answer("1.0.0", false, false), own), Some(Version(1, 0, 0)));
        assert_eq!(newer(&answer("v0.8.1", false, false), own), None);
        assert_eq!(newer(&answer("v0.8.0", false, false), own), None);
        assert_eq!(newer(&answer("v0.9.0", true, false), own), None);
        assert_eq!(newer(&answer("v0.9.0", false, true), own), None);
        assert_eq!(newer(&answer("v0.9.0-rc1", false, false), own), None);
        assert_eq!(newer(br#"{"tag_name": 9}"#, own), None);
        assert_eq!(newer(br#"{"name": "v0.9.0"}"#, own), None);
        assert_eq!(newer(b"kein json", own), None);
        assert_eq!(newer(b"", own), None);
        assert_eq!(newer(br#"{"tag_name": "v0.9.0"}"#, own), Some(Version(0, 9, 0)));
    }

    #[test]
    fn zu_grosse_antwort_wird_nicht_ausgewertet() {
        let mut big = answer("v9.0.0", false, false);
        big.pop();
        let padding = usize::try_from(MAX_ANSWER).unwrap();
        big.extend(format!(r#","x":"{}"}}"#, "x".repeat(padding)).bytes());
        assert!(serde_json::from_slice::<serde_json::Value>(&big).is_ok());
        assert_eq!(newer(&big, Version(0, 8, 1)), None);
    }

    #[test]
    fn zeitplan() {
        let start = Instant::now();
        let mut schedule = Schedule::default();
        assert!(schedule.due(start), "beim Start fällig");
        schedule.done(start);
        assert!(!schedule.due(start + Duration::from_secs(60)));
        assert!(!schedule.due(start + Duration::from_mins(23 * 60 + 59)));
        assert!(schedule.due(start + INTERVAL));
        schedule.done(start + INTERVAL);
        assert!(!schedule.due(start + INTERVAL + Duration::from_secs(1)));
        schedule.enable();
        assert!(
            schedule.due(start + INTERVAL + Duration::from_secs(1)),
            "Einschalten macht sofort fällig"
        );
    }

    #[test]
    fn dieselbe_version_nur_einmal() {
        let mut schedule = Schedule::default();
        assert_eq!(schedule.shown(), None);
        assert!(schedule.record(Version(0, 9, 0)));
        assert!(!schedule.record(Version(0, 9, 0)));
        assert!(!schedule.record(Version(0, 8, 9)));
        assert!(schedule.record(Version(0, 10, 0)));
        assert_eq!(schedule.shown(), Some(Version(0, 10, 0)));
    }

    #[test]
    fn adressen_des_auslieferungsrepositorys() {
        for url in [RELEASES_URL, LATEST_API] {
            assert!(url.starts_with("https://"), "{url}");
            assert!(url.contains("derHouwer/celata"), "{url}");
            assert!(!url.contains(concat!("celata", "-dev")), "{url}");
        }
        assert_eq!(MAX_ANSWER, 256 * 1024);
    }

    #[test]
    fn nie_an_der_sperre_vorbei() {
        for display in [
            DisplayState::Blocked,
            DisplayState::ConnectingBlocked,
            DisplayState::ServiceUnreachableBlocked,
        ] {
            assert!(!may_check(Some(&display), &Ok(false)), "{display:?}");
        }
        let free = DisplayState::NotConnected;
        assert!(may_check(Some(&free), &Ok(false)));
        assert!(may_check(None, &Ok(false)));
        assert!(!may_check(Some(&free), &Ok(true)));
        assert!(!may_check(Some(&free), &Err(io::ErrorKind::NotFound.into())));
        let connected = DisplayState::Connected { server: None };
        assert!(may_check(Some(&connected), &Ok(false)));
    }

    #[test]
    fn keine_anfrage_bei_sperre_oder_unlesbaren_routen() {
        // Returns before any request: no network in this test.
        assert_eq!(fetch_latest(|| Ok(true)), None);
        assert_eq!(fetch_latest(|| Err(io::ErrorKind::NotFound.into())), None);
    }
}
