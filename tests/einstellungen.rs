// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! Contract tests for the settings of the service, against an imitated service. The real service, the
//! VPN and the internet stay untouched.
#![forbid(unsafe_code)]

mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use celata::daemon::{Client, Timing};
use celata::settings::{Applied, Class, Filter, Ledger, Settings};
use celata::state::{ConnectResult, RouteResult, run_connect, run_route};
use celata::switch::{Phase, SwitchResult, Target, run_reconnect};
use support::{FakeDaemon, Reply, result, state_json};

const RUN: &str = "0123456789abcdef0123456789abcdef";
const NL: &str = "nl.hideservers.net";
const DE: &str = "de.hideservers.net";

/// Forbidden calls.
const FORBIDDEN: [&str; 8] = [
    "GET /configuration",
    "GET /configureConnect",
    "POST /configureConnect",
    "GET /token",
    "GET /requestToken",
    "GET /log",
    "GET /serverList",
    "GET /shutdown",
];

fn fast_timing() -> Timing {
    Timing {
        call: Duration::from_secs(2),
        connect: Duration::from_secs(2),
        destroy_total: Duration::from_millis(100),
        destroy_pause: Duration::from_millis(5),
        retry: Duration::from_millis(10),
    }
}

/// Service with version `version`; `/connect` fails on the first `failures` attempts.
fn daemon(version: &'static str, failures: u32) -> FakeDaemon {
    let attempts = Arc::new(AtomicU32::new(0));
    FakeDaemon::start(move |call| match call {
        "GET /version" => Reply::Text(version.to_owned()),
        "POST /configuration" => Reply::Json(r#"{"result":true}"#.to_owned()),
        "GET /connect" => {
            if attempts.fetch_add(1, Ordering::Relaxed) < failures {
                Reply::Json(r#"{"error":{"code":"connect","message":"timeout"}}"#.to_owned())
            } else {
                Reply::Json(result(&state_json("connected", NL)))
            }
        }
        "GET /disconnect" => Reply::Json(result(&state_json("routed", NL))),
        "GET /destroy" => Reply::Json(result(&state_json("clean", ""))),
        _ => Reply::Status(404),
    })
}

/// Service that rejects every setting with HTTP 400.
fn rejecting_daemon() -> FakeDaemon {
    FakeDaemon::start(|call| match call {
        "GET /version" => Reply::Text("0.9.14".to_owned()),
        "POST /configuration" => Reply::StatusJson(
            400,
            r#"{"error":{"code":"configuration","message":"json: cannot unmarshal string into Go value"}}"#.to_owned(),
        ),
        _ => Reply::Json(result(&state_json("connected", NL))),
    })
}

fn client(daemon: &FakeDaemon) -> Client {
    Client::with_timing(&daemon.path, fast_timing())
}

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

fn fresh_ledger() -> Ledger {
    Ledger::new(Applied::default(), Some(RUN.to_owned()))
}

/// Applied state after a connection with `settings` in this run of the service.
fn ledger_after(settings: &Settings) -> Ledger {
    Ledger::new(
        Applied {
            invocation: Some(RUN.to_owned()),
            sent: Some(settings.clone()),
            routes_built_with: Some(settings.clone()),
        },
        Some(RUN.to_owned()),
    )
}

fn assert_nothing_forbidden(daemon: &FakeDaemon) {
    for call in daemon.calls() {
        assert!(!FORBIDDEN.contains(&call.as_str()), "verboten: {call}");
    }
}

fn reconnect(daemon: &FakeDaemon, ledger: &mut Ledger, from: Settings, to: Settings) -> SwitchResult {
    let class = celata::settings::changed_class(&from, &to).unwrap_or(Class::Reconnect);
    run_reconnect(
        &client(daemon),
        ledger,
        &Target::new(NL, from),
        &Target::new(NL, to),
        class,
        || Ok(true),
        |_| true,
        |_: Phase| true,
    )
}

fn json(body: &str) -> serde_json::Value {
    serde_json::from_str(body).unwrap()
}

// --- Promise 1: defaults stay defaults ---

#[test]
fn standard_verbinden_ruft_dasselbe_wie_funktion_003() {
    let daemon = daemon("0.9.14", 0);
    let target = Target::new(NL, Settings::default());
    let result = run_connect(&client(&daemon), &mut fresh_ledger(), &target, true, false);
    assert_eq!(result, ConnectResult::Connected);
    assert_eq!(daemon.calls(), ["POST /configuration", "GET /connect"]);
    assert_eq!(daemon.bodies()[0], r#"{"Rest":{"Host":"nl.hideservers.net"}}"#);
}

#[test]
fn standard_wechsel_ruft_dasselbe_wie_funktion_003() {
    let daemon = daemon("0.9.14", 0);
    let result = run_reconnect(
        &client(&daemon),
        &mut fresh_ledger(),
        &Target::new(NL, Settings::default()),
        &Target::new(DE, Settings::default()),
        Class::Reconnect,
        || Ok(true),
        |_| true,
        |_| true,
    );
    assert_eq!(result, SwitchResult::Switched);
    assert_eq!(
        daemon.calls(),
        ["GET /disconnect", "POST /configuration", "GET /connect"]
    );
    assert_eq!(daemon.bodies()[1], r#"{"Rest":{"Host":"de.hideservers.net"}}"#);
}

// --- Promise 2: complete set ---

#[test]
fn abweichende_einstellungen_senden_den_vollen_satz() {
    let daemon = daemon("0.9.14", 0);
    let target = Target::new(NL, ads());
    assert_eq!(
        run_connect(&client(&daemon), &mut fresh_ledger(), &target, true, false),
        ConnectResult::Connected
    );
    assert_eq!(daemon.calls(), ["GET /version", "POST /configuration", "GET /connect"]);
    let sent = json(&daemon.bodies()[1]);
    assert_eq!(sent["Rest"]["Host"], NL);
    assert_eq!(sent["Rest"]["Filter"]["ads"], true);
    assert_eq!(sent["Rest"]["RestTimeout"], 90_000_000_000u64);
    assert_eq!(sent["WireGuard"]["DpdTimeout"], 60_000_000_000u64);
    assert_eq!(sent["Rest"]["Mark"], sent["WireGuard"]["Mark"]);
    for section in ["Rest", "WireGuard", "DoH", "Plain"] {
        assert!(sent.get(section).is_some(), "{section}");
    }
    let lower = daemon.bodies()[1].to_ascii_lowercase();
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
        assert!(!lower.contains(forbidden), "{forbidden}");
    }
    assert_nothing_forbidden(&daemon);
}

#[test]
fn zurueck_auf_standard_sendet_den_vollen_standardsatz() {
    let daemon = daemon("0.9.14", 0);
    let mut ledger = ledger_after(&ads());
    let target = Target::new(NL, Settings::default());
    assert_eq!(
        run_connect(&client(&daemon), &mut ledger, &target, true, false),
        ConnectResult::Connected
    );
    assert_eq!(daemon.calls(), ["POST /configuration", "GET /connect"]);
    assert_eq!(json(&daemon.bodies()[0])["Rest"]["Filter"]["ads"], false);
    // After that the defaults apply in the service again: next time the host is enough.
    assert!(!ledger.send_full(&Settings::default()));
}

// --- Promise 3: version ---

#[test]
fn abweichende_einstellungen_nur_an_version_0_9_14() {
    let daemon = daemon("0.9.15", 0);
    let target = Target::new(NL, ads());
    let result = run_connect(&client(&daemon), &mut fresh_ledger(), &target, true, false);
    assert_eq!(result, ConnectResult::Untested("0.9.15".to_owned()));
    assert_eq!(daemon.calls(), ["GET /version"]);
    let mut ledger = ledger_after(&Settings::default());
    assert_eq!(
        reconnect(&daemon, &mut ledger, Settings::default(), ads()),
        SwitchResult::Untested("0.9.15".to_owned())
    );
    assert_eq!(daemon.calls(), ["GET /version", "GET /version"]);
    // The tested version is in the notice, in every language.
    for language in celata::i18n::Language::ALL {
        let notice = celata::settings::untested_notice("9.9.9").render(language);
        assert!(notice.contains("0.9.14") && notice.contains("9.9.9"), "{notice}");
    }
}

#[test]
fn standardeinstellungen_gehen_auch_an_andere_versionen() {
    let daemon = daemon("0.9.15", 0);
    let target = Target::new(NL, Settings::default());
    assert_eq!(
        run_connect(&client(&daemon), &mut fresh_ledger(), &target, true, false),
        ConnectResult::Connected
    );
    assert_eq!(daemon.calls(), ["POST /configuration", "GET /connect"]);
}

// --- Promise 4: reconnecting ---

#[test]
fn neu_verbinden_schickt_einstellungen_vor_dem_trennen() {
    let daemon = daemon("0.9.14", 0);
    let mut ledger = ledger_after(&Settings::default());
    assert_eq!(
        reconnect(&daemon, &mut ledger, Settings::default(), ads()),
        SwitchResult::Switched
    );
    assert_eq!(
        daemon.calls(),
        ["GET /version", "POST /configuration", "GET /disconnect", "GET /connect"]
    );
    assert_eq!(json(&daemon.bodies()[1])["Rest"]["Filter"]["ads"], true);
    assert_eq!(ledger.applied.sent, Some(ads()));
}

#[test]
fn neu_verbinden_nur_mit_bereiter_sperre() {
    let daemon = daemon("0.9.14", 0);
    let result = run_reconnect(
        &client(&daemon),
        &mut ledger_after(&Settings::default()),
        &Target::new(NL, Settings::default()),
        &Target::new(NL, ads()),
        Class::Reconnect,
        || Ok(false),
        |_| true,
        |_| true,
    );
    assert_eq!(result, SwitchResult::Refused);
    assert_eq!(daemon.calls(), ["GET /version"]);
}

#[test]
fn ohne_eingeschaltete_sperre_wird_trotzdem_neu_verbunden() {
    let daemon = daemon("0.9.14", 0);
    let from = no_kill_switch();
    let to = Settings {
        filter: Filter {
            ads: true,
            ..Filter::default()
        },
        ..no_kill_switch()
    };
    let result = run_reconnect(
        &client(&daemon),
        &mut ledger_after(&from),
        &Target::new(NL, from),
        &Target::new(NL, to),
        Class::Reconnect,
        || Ok(false),
        |_| true,
        |_| true,
    );
    assert_eq!(result, SwitchResult::Switched);
}

#[test]
fn neuaufbau_ruft_destroy_statt_disconnect() {
    let daemon = daemon("0.9.14", 0);
    let mut ledger = ledger_after(&Settings::default());
    assert_eq!(
        reconnect(&daemon, &mut ledger, Settings::default(), no_kill_switch()),
        SwitchResult::Switched
    );
    assert_eq!(
        daemon.calls(),
        ["GET /version", "POST /configuration", "GET /destroy", "GET /connect"]
    );
    assert_eq!(json(&daemon.bodies()[1])["WireGuard"]["LeakProtection"], false);
    assert_eq!(ledger.applied.routes_built_with, Some(no_kill_switch()));
}

// --- Promise 5: way back ---

#[test]
fn rueckweg_bei_neu_verbinden() {
    let daemon = daemon("0.9.14", 1);
    let mut ledger = ledger_after(&Settings::default());
    let result = reconnect(&daemon, &mut ledger, Settings::default(), ads());
    assert!(matches!(result, SwitchResult::Returned(_)), "{result:?}");
    assert_eq!(
        daemon.calls(),
        [
            "GET /version",
            "POST /configuration",
            "GET /disconnect",
            "GET /connect",
            "POST /configuration",
            "GET /connect"
        ]
    );
    // Way back with the old settings; the service already held different ones, hence the full set.
    assert_eq!(json(&daemon.bodies()[4])["Rest"]["Filter"]["ads"], false);
    assert_eq!(ledger.applied.sent, Some(Settings::default()));
}

#[test]
fn rueckweg_bei_neuaufbau() {
    let daemon = daemon("0.9.14", 1);
    let mut ledger = ledger_after(&Settings::default());
    let result = reconnect(&daemon, &mut ledger, Settings::default(), no_kill_switch());
    assert!(matches!(result, SwitchResult::Returned(_)), "{result:?}");
    assert_eq!(
        daemon.calls(),
        [
            "GET /version",
            "POST /configuration",
            "GET /destroy",
            "GET /connect",
            "POST /configuration",
            "GET /destroy",
            "GET /connect"
        ]
    );
    assert_eq!(json(&daemon.bodies()[4])["WireGuard"]["LeakProtection"], true);
    assert_eq!(ledger.applied.routes_built_with, Some(Settings::default()));
}

#[test]
fn gescheiterter_rueckweg_ruft_kein_destroy_bei_neu_verbinden() {
    let daemon = daemon("0.9.14", 2);
    let result = reconnect(
        &daemon,
        &mut ledger_after(&Settings::default()),
        Settings::default(),
        ads(),
    );
    assert!(matches!(result, SwitchResult::Stranded(_)), "{result:?}");
    assert!(!daemon.calls().contains(&"GET /destroy".to_owned()));
}

// --- Promises 5a and 6: rejection ---

#[test]
fn abgelehnte_einstellung_trennt_nicht() {
    let daemon = rejecting_daemon();
    for to in [ads(), no_kill_switch()] {
        let before = daemon.calls().len();
        let result = reconnect(
            &daemon,
            &mut ledger_after(&Settings::default()),
            Settings::default(),
            to,
        );
        assert_eq!(
            result,
            SwitchResult::Rejected("json: cannot unmarshal string into Go value".to_owned())
        );
        assert_eq!(daemon.calls()[before..], ["GET /version", "POST /configuration"]);
    }
}

#[test]
fn abgelehnte_einstellung_verbindet_nicht() {
    let daemon = rejecting_daemon();
    let target = Target::new(NL, ads());
    let result = run_connect(&client(&daemon), &mut fresh_ledger(), &target, true, false);
    assert!(matches!(result, ConnectResult::Rejected(_)), "{result:?}");
    assert!(!daemon.calls().contains(&"GET /connect".to_owned()));
}

// --- Promise 7: blocked stays blocked, the next connection rebuilds ---

#[test]
fn verbinden_aus_der_sperre_baut_neu_auf() {
    let daemon = daemon("0.9.14", 0);
    let mut ledger = ledger_after(&Settings::default());
    assert!(ledger.needs_rebuild(&no_kill_switch()));
    let target = Target::new(NL, no_kill_switch());
    assert_eq!(
        run_connect(&client(&daemon), &mut ledger, &target, false, true),
        ConnectResult::Connected
    );
    assert_eq!(
        daemon.calls(),
        ["GET /version", "POST /configuration", "GET /destroy", "GET /connect"]
    );
    assert!(!ledger.needs_rebuild(&no_kill_switch()));
}

#[test]
fn ohne_neuaufbau_bleibt_die_sperre_beim_verbinden() {
    let daemon = daemon("0.9.14", 0);
    let mut ledger = ledger_after(&Settings::default());
    let target = Target::new(NL, ads());
    assert_eq!(
        run_connect(&client(&daemon), &mut ledger, &target, false, false),
        ConnectResult::Connected
    );
    assert!(!daemon.calls().contains(&"GET /destroy".to_owned()));
}

#[test]
fn aus_freiem_zustand_wird_nie_zusaetzlich_zerstoert() {
    let daemon = daemon("0.9.14", 0);
    let target = Target::new(NL, no_kill_switch());
    assert_eq!(
        run_connect(&client(&daemon), &mut fresh_ledger(), &target, true, true),
        ConnectResult::Connected
    );
    assert_eq!(daemon.calls(), ["GET /version", "POST /configuration", "GET /connect"]);
}

// --- Promise 8: no keys; promise 9: `/externalIps` only as a call of its own ---

#[test]
fn details_ohne_schluessel() {
    let daemon = FakeDaemon::start(|call| match call {
        "GET /state" => Reply::Json(result(&support::connected_json_with_details(NL))),
        "GET /externalIps" => Reply::Text(String::new()),
        _ => Reply::Status(404),
    });
    let client = client(&daemon);
    let state = client.state().unwrap();
    assert_eq!(state.rx, Some(12_345_678));
    assert_eq!(state.tx, Some(2_345_678));
    assert_eq!(state.since.as_deref(), Some("2026-11-09T12:34:56.123456789+01:00"));
    assert_eq!(state.addresses, ["10.128.0.2", "fd00::2"]);
    assert_eq!(state.dns, ["10.128.0.1"]);
    let shown = format!("{state:?}");
    assert!(!shown.contains(support::SECRET_005));
    assert!(!shown.contains("203.0.113.7"), "endpoint wird nicht gelesen");
    client.external_ips().unwrap();
    assert_eq!(daemon.calls(), ["GET /state", "GET /externalIps"]);
}

#[test]
fn ergebnis_von_external_ips_kommt_ueber_watch() {
    let line = support::external_ips_json("203.0.113.9", "2001:db8::9");
    let daemon = FakeDaemon::start(move |call| match call {
        "GET /watch" => Reply::Lines(vec![line.clone()]),
        _ => Reply::Status(404),
    });
    let state = client(&daemon).watch().unwrap().next_state().unwrap().unwrap();
    assert_eq!(state.code, "external ips");
    assert_eq!(state.external_v4.as_deref(), Some("203.0.113.9"));
    assert_eq!(state.external_v6.as_deref(), Some("2001:db8::9"));
}

// --- Promise 9: blocking the internet ---

fn route_daemon(version: &'static str) -> FakeDaemon {
    FakeDaemon::start(move |call| match call {
        "GET /version" => Reply::Text(version.to_owned()),
        "POST /configuration" => Reply::Json(r#"{"result":true}"#.to_owned()),
        "GET /route" => Reply::Json(result(&state_json("routed", ""))),
        _ => Reply::Status(404),
    })
}

#[test]
fn sperren_mit_standard_ruft_nur_route() {
    let daemon = route_daemon("0.9.14");
    let mut ledger = fresh_ledger();
    assert_eq!(
        run_route(&client(&daemon), &mut ledger, &Settings::default()),
        RouteResult::Routed
    );
    assert_eq!(daemon.calls(), ["GET /route"]);
    assert_eq!(ledger.applied.routes_built_with, Some(Settings::default()));
}

#[test]
fn sperren_schickt_vorher_die_einstellungen_ohne_host() {
    let daemon = route_daemon("0.9.14");
    let settings = Settings {
        split_tunnel: vec!["192.168.1.0/24".to_owned()],
        ..Settings::default()
    };
    assert_eq!(
        run_route(&client(&daemon), &mut fresh_ledger(), &settings),
        RouteResult::Routed
    );
    assert_eq!(daemon.calls(), ["GET /version", "POST /configuration", "GET /route"]);
    let sent = json(&daemon.bodies()[1]);
    assert!(sent["Rest"].get("Host").is_none());
    assert_eq!(sent["WireGuard"]["SplitTunnel"], "192.168.1.0/24");
    assert_nothing_forbidden(&daemon);
}

#[test]
fn sperren_ohne_eingeschaltete_sperre_ruft_nichts() {
    let daemon = route_daemon("0.9.14");
    let result = run_route(&client(&daemon), &mut fresh_ledger(), &no_kill_switch());
    assert_eq!(result, RouteResult::Disabled);
    assert_eq!(daemon.calls(), [] as [std::string::String; 0]);
}

#[test]
fn sperren_bei_abgelehnten_einstellungen_oder_fremder_version() {
    let daemon = rejecting_daemon();
    let result = run_route(&client(&daemon), &mut fresh_ledger(), &ads());
    assert!(matches!(result, RouteResult::Rejected(_)), "{result:?}");
    assert!(!daemon.calls().contains(&"GET /route".to_owned()));
    let daemon = route_daemon("0.9.15");
    assert_eq!(
        run_route(&client(&daemon), &mut fresh_ledger(), &ads()),
        RouteResult::Untested("0.9.15".to_owned())
    );
    assert_eq!(daemon.calls(), ["GET /version"]);
}

// --- Hidden settings ---

/// Creates a settings file as an older version wrote it (the filters are part of the settings).
fn settings_file(content: &str) -> std::path::PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let number = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("celata-vertrag-{}-{number}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("einstellungen.json");
    std::fs::write(&path, content).unwrap();
    path
}

#[test]
fn datei_mit_filtern_schickt_keine_filter() {
    let path = settings_file(
        r#"{"version":1,"settings":{"filter":{"ads":true,"force_dns":true},"split_tunnel":["192.168.1.0/24"]}}"#,
    );
    let loaded = celata::settings::load(Some(&path));
    assert_eq!(loaded.notice, None);
    let daemon = daemon("0.9.14", 0);
    let target = Target::new(NL, loaded.settings);
    assert_eq!(
        run_connect(&client(&daemon), &mut fresh_ledger(), &target, true, false),
        ConnectResult::Connected
    );
    assert_eq!(daemon.calls(), ["GET /version", "POST /configuration", "GET /connect"]);
    let body = &daemon.bodies()[1];
    let sent = json(body);
    assert_eq!(sent["WireGuard"]["SplitTunnel"], "192.168.1.0/24");
    assert_eq!(sent["Rest"]["Filter"]["ads"], false);
    assert_eq!(sent["Rest"]["Filter"]["forceDns"], false);
    assert!(!body.contains(r#""ads":true"#) && !body.contains(r#""forceDns":true"#));
    assert_nothing_forbidden(&daemon);
}

#[test]
fn datei_nur_mit_filtern_schickt_nur_den_host() {
    let path = settings_file(r#"{"version":1,"settings":{"filter":{"ads":true}}}"#);
    let loaded = celata::settings::load(Some(&path));
    assert_eq!(loaded.notice, None);
    let daemon = daemon("0.9.14", 0);
    let target = Target::new(NL, loaded.settings);
    assert_eq!(
        run_connect(&client(&daemon), &mut fresh_ledger(), &target, true, false),
        ConnectResult::Connected
    );
    // As with nothing but defaults.
    assert_eq!(daemon.calls(), ["POST /configuration", "GET /connect"]);
    assert_eq!(daemon.bodies()[0], r#"{"Rest":{"Host":"nl.hideservers.net"}}"#);
    assert_nothing_forbidden(&daemon);
}

// --- Switch "Show donation hint" ---

#[test]
fn spendenschalter_aendert_nichts_am_auftrag_an_den_dienst() {
    let file = |hint: bool| {
        settings_file(&format!(
            r#"{{"version":1,"settings":{{"split_tunnel":["192.168.1.0/24"],"port":433}},"app":{{"donation_hint":{hint}}}}}"#
        ))
    };
    let mut seen = Vec::new();
    for hint in [false, true] {
        let loaded = celata::settings::load(Some(&file(hint)));
        assert_eq!(loaded.notice, None);
        assert_eq!(loaded.app.donation_hint, hint);
        let daemon = daemon("0.9.14", 0);
        let target = Target::new(NL, loaded.settings);
        assert_eq!(
            run_connect(&client(&daemon), &mut fresh_ledger(), &target, true, false),
            ConnectResult::Connected
        );
        for body in daemon.bodies() {
            assert!(!body.contains("donation"), "Schalter darf nicht an den Dienst: {body}");
        }
        assert_nothing_forbidden(&daemon);
        seen.push((daemon.calls(), daemon.bodies()));
    }
    assert_eq!(
        seen[0], seen[1],
        "Mit und ohne Spendenhinweis bekommt der Dienst genau dasselbe"
    );
}

// --- Stored language ---

#[test]
fn sprachwahl_aendert_nichts_am_auftrag_an_den_dienst() {
    let file = |language: &str| {
        settings_file(&format!(
            r#"{{"version":1,"settings":{{"split_tunnel":["192.168.1.0/24"],"port":433}},"app":{{"language":"{language}"}}}}"#
        ))
    };
    let mut seen = Vec::new();
    for (code, language) in [("de", celata::i18n::Language::De), ("en", celata::i18n::Language::En)] {
        let loaded = celata::settings::load(Some(&file(code)));
        assert_eq!(loaded.notice, None);
        assert_eq!(loaded.app.language, language);
        let daemon = daemon("0.9.14", 0);
        let target = Target::new(NL, loaded.settings);
        assert_eq!(
            run_connect(&client(&daemon), &mut fresh_ledger(), &target, true, false),
            ConnectResult::Connected
        );
        for body in daemon.bodies() {
            assert!(!body.contains("language"), "Sprache darf nicht an den Dienst: {body}");
        }
        assert_nothing_forbidden(&daemon);
        seen.push((daemon.calls(), daemon.bodies()));
    }
    assert_eq!(
        seen[0], seen[1],
        "Mit Deutsch und Englisch bekommt der Dienst genau dasselbe"
    );
}
