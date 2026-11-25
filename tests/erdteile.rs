// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! Check for the maintainer: does the continent table of Celata agree with the server list, and does the list
//! hold a country the table does not know? Runs on the stored test list; with `CELATA_SERVER_LIST` set it reads
//! that file instead, for example `~/.cache/celata/servers.json` (public data, no credentials):
//!
//! ```text
//! CELATA_SERVER_LIST=~/.cache/celata/servers.json cargo test --test erdteile -- --nocapture
//! ```

use std::fs;

use celata::continent::{self, Continent};
use celata::i18n::Language;
use celata::servers::{self, Country};

const TEST_LIST: &[u8] = include_bytes!("fixtures/servers-2026-11-19.json");

/// A server list as the program stores it (`{"fetched_at": …, "data": […]}`) or as the service delivers it
/// (a bare list).
fn countries_of(bytes: &[u8]) -> Result<Vec<Country>, String> {
    if let Ok(countries) = servers::parse_list(bytes) {
        return Ok(countries);
    }
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|error| format!("not JSON: {error}"))?;
    let data = value.get("data").ok_or("neither a server list nor a stored list")?;
    let list = serde_json::to_vec(data).map_err(|error| error.to_string())?;
    servers::parse_list(&list).map_err(|_| "not a valid server list".to_owned())
}

fn name(continent: Continent) -> &'static str {
    continent.name(Language::En)
}

/// One line per finding; empty when the table and the list agree and every country is known.
fn findings(countries: &[Country]) -> Vec<String> {
    let mut lines: Vec<String> = continent::deviations(countries)
        .iter()
        .map(|found| {
            format!(
                "{} {}: own {}, list {}",
                found.code,
                found.name,
                name(found.own),
                name(found.list)
            )
        })
        .collect();
    lines.extend(continent::unknown(countries).iter().map(|found| {
        format!(
            "{} {}: not in the table, assigned {} ({})",
            found.code,
            found.name,
            name(found.assigned),
            found.step.word()
        )
    }));
    lines
}

fn report(countries: &[Country]) {
    let lines = findings(countries);
    assert!(
        lines.is_empty(),
        "The continent table and the server list differ:\n{}",
        lines.join("\n")
    );
    println!("no deviations, no unknown countries");
}

#[test]
fn the_list_agrees_with_the_table() {
    let (label, bytes) = match std::env::var_os("CELATA_SERVER_LIST") {
        Some(path) => {
            let bytes = fs::read(&path).unwrap_or_else(|error| {
                panic!("CELATA_SERVER_LIST {path:?} cannot be read: {error}");
            });
            (format!("{path:?}"), bytes)
        }
        None => ("the stored test list".to_owned(), TEST_LIST.to_vec()),
    };
    let countries = countries_of(&bytes).unwrap_or_else(|reason| panic!("{label} is no server list: {reason}"));
    assert!(!countries.is_empty(), "{label} holds no country");
    println!("checked {} countries of {label}", countries.len());
    report(&countries);
}

#[test]
fn the_stored_test_list_has_57_countries_without_findings() {
    let countries = countries_of(TEST_LIST).unwrap();
    assert_eq!(countries.len(), 57);
    assert!(findings(&countries).is_empty());
}

#[test]
fn findings_are_reported_in_the_documented_form() {
    let list = br#"[
      {"hostname": "de-v4.hideservers.net", "displayName": "Germany", "geo": {"countryCode": "DE", "continent": "AS"}},
      {"hostname": "xx-v4.hideservers.net", "displayName": "Xland", "geo": {"countryCode": "XX", "continent": "AF"}},
      {"hostname": "yy-v4.hideservers.net", "displayName": "Yland", "geo": {"countryCode": "YY"}}
    ]"#;
    let countries = servers::parse_list(list).unwrap();
    assert_eq!(
        findings(&countries),
        [
            "DE Germany: own Europe, list Asia",
            "XX Xland: not in the table, assigned Africa (from the list)",
            "YY Yland: not in the table, assigned Europe (fallback)",
        ]
    );
}

#[test]
fn a_stored_list_is_read_like_a_bare_list() {
    let stored = format!(
        "{{\"fetched_at\": 1, \"data\": {}}}",
        std::str::from_utf8(TEST_LIST).unwrap()
    );
    let countries = countries_of(stored.as_bytes()).unwrap();
    assert_eq!(countries.len(), 57);
    assert!(countries_of(b"{\"x\": 1}").is_err());
    assert!(countries_of(b"nonsense").is_err());
}
