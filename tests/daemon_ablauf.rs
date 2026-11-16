// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! Sequences against an imitated service. The real service, the VPN and the internet stay untouched.
#![forbid(unsafe_code)]

mod support;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use celata::daemon::{Client, Error, Flow, Observation, Timing};
use celata::settings::{Ledger, Settings};
use celata::state::{
    ConnectResult, DisconnectStart, DisplayState, Monitor, STOP_COMMAND, run_connect, run_disconnect, run_unblock,
};
use celata::switch::{Phase, SwitchResult, Target, run_switch};
use support::{FAKE_SECRET, FakeDaemon, Reply, result, state_json, unique_path};

const ALLOWED: [&str; 7] = [
    "GET /version",
    "GET /state",
    "GET /watch",
    "POST /configuration",
    "GET /connect",
    "GET /destroy",
    // Only within a switch of servers.
    "GET /disconnect",
];

/// Connecting with default settings.
fn connect_default(client: &Client, host: &str, was_free: bool) -> ConnectResult {
    run_connect(
        client,
        &mut Ledger::default(),
        &Target::new(host, Settings::default()),
        was_free,
        false,
    )
}

fn fast_timing() -> Timing {
    Timing {
        call: Duration::from_secs(2),
        connect: Duration::from_secs(2),
        destroy_total: Duration::from_millis(100),
        destroy_pause: Duration::from_millis(5),
        retry: Duration::from_millis(10),
    }
}

#[expect(
    clippy::match_same_arms,
    reason = "one line per call of the service, as in the list of allowed calls"
)]
fn normal_daemon() -> FakeDaemon {
    FakeDaemon::start(|call| match call {
        "GET /version" => Reply::Text("0.9.14".to_owned()),
        "GET /state" => Reply::Json(result(&state_json("clean", ""))),
        "GET /watch" => Reply::Lines(vec![state_json("connecting", "nl.hideservers.net")]),
        "POST /configuration" => Reply::Json(r#"{"result":true}"#.to_owned()),
        "GET /connect" => Reply::Json(result(&state_json("connected", "nl.hideservers.net"))),
        "GET /destroy" => Reply::Json(result(&state_json("clean", ""))),
        "GET /disconnect" => Reply::Json(result(&state_json("routed", "nl.hideservers.net"))),
        _ => Reply::Status(404),
    })
}

#[test]
fn nur_erlaubte_pfade_werden_aufgerufen() {
    let daemon = normal_daemon();
    let client = Client::with_timing(&daemon.path, fast_timing());

    assert_eq!(client.version().unwrap(), "0.9.14");
    assert_eq!(client.state().unwrap().code, "clean");
    client.watch().unwrap().next_state().unwrap();
    client
        .configure(&Settings::default(), "nl.hideservers.net", false)
        .unwrap();
    assert_eq!(client.connect().unwrap().code, "connected");
    assert_eq!(client.destroy().unwrap().code, "clean");
    assert_eq!(client.disconnect().unwrap().code, "routed");

    let calls = daemon.calls();
    assert_eq!(calls.len(), ALLOWED.len());
    for call in &calls {
        assert!(ALLOWED.contains(&call.as_str()), "nicht erlaubt: {call}");
    }
}

#[test]
fn disconnect_bei_http_409_heisst_beschaeftigt_ohne_wiederholung() {
    let daemon = FakeDaemon::start(|_| Reply::Status(409));
    let client = Client::with_timing(&daemon.path, fast_timing());
    assert_eq!(client.disconnect(), Err(Error::Busy));
    assert_eq!(daemon.calls(), vec!["GET /disconnect"]);
}

#[test]
fn standardeinstellungen_senden_nur_den_hostnamen() {
    let daemon = normal_daemon();
    Client::with_timing(&daemon.path, fast_timing())
        .configure(&Settings::default(), "nl.hideservers.net", false)
        .unwrap();
    assert_eq!(
        daemon.bodies(),
        vec![r#"{"Rest":{"Host":"nl.hideservers.net"}}"#.to_owned()]
    );
}

#[test]
fn ungueltiger_hostname_erreicht_den_dienst_nicht() {
    let daemon = normal_daemon();
    let client = Client::with_timing(&daemon.path, fast_timing());
    assert_eq!(
        client.configure(&Settings::default(), "x\",\"Password\":\"y", false),
        Err(Error::InvalidHost)
    );
    assert_eq!(daemon.calls(), [] as [std::string::String; 0]);
}

#[test]
fn schluesselwerte_tauchen_in_keiner_ausgabe_auf() {
    let daemon = normal_daemon();
    let client = Client::with_timing(&daemon.path, fast_timing());

    let state = client.connect().unwrap();
    let event = client.watch().unwrap().next_state().unwrap().unwrap();

    assert_eq!(state.host.as_deref(), Some("nl.hideservers.net"));
    for text in [format!("{state:?}"), format!("{event:?}")] {
        assert!(!text.contains(FAKE_SECRET), "{text}");
    }
}

#[test]
fn fehlender_socket_heisst_dienst_laeuft_nicht() {
    let client = Client::with_timing(unique_path(), fast_timing());
    assert_eq!(client.state(), Err(Error::NotRunning));
    assert_eq!(client.destroy(), Err(Error::NotRunning));
}

#[test]
fn fehlende_rechte_werden_erkannt() {
    if !support::permissions_are_enforced() {
        return;
    }
    let path = unique_path();
    let _listener = UnixListener::bind(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
    assert_eq!(
        Client::with_timing(&path, fast_timing()).state(),
        Err(Error::PermissionDenied)
    );
}

#[test]
fn dienstfehler_wird_weitergegeben() {
    let daemon =
        FakeDaemon::start(|_| Reply::Json(r#"{"error":{"code":"connect","message":"no such host"}}"#.to_owned()));
    let client = Client::with_timing(&daemon.path, fast_timing());
    assert_eq!(client.connect(), Err(Error::Service("no such host".to_owned())));
}

#[test]
fn unlesbare_antwort_ist_ein_fehler() {
    let daemon = FakeDaemon::start(|_| Reply::Json("kein json".to_owned()));
    assert_eq!(
        Client::with_timing(&daemon.path, fast_timing()).state(),
        Err(Error::InvalidResponse)
    );
}

#[test]
fn destroy_wiederholt_solange_der_dienst_beschaeftigt_ist() {
    let attempts = Arc::new(AtomicU32::new(0));
    let counter = attempts.clone();
    let daemon = FakeDaemon::start(move |_| {
        if counter.fetch_add(1, Ordering::SeqCst) < 3 {
            Reply::Status(409)
        } else {
            Reply::Json(result(&state_json("clean", "")))
        }
    });
    let client = Client::with_timing(&daemon.path, fast_timing());

    assert_eq!(client.destroy().unwrap().code, "clean");
    assert_eq!(attempts.load(Ordering::SeqCst), 4);
}

#[test]
fn destroy_gibt_nach_dem_limit_auf() {
    let daemon = FakeDaemon::start(|_| Reply::Status(409));
    let client = Client::with_timing(&daemon.path, fast_timing());
    assert_eq!(client.destroy(), Err(Error::Busy));
    assert!(daemon.calls().len() > 1);
}

#[test]
fn andere_aufrufe_wiederholen_bei_409_nicht() {
    let daemon = FakeDaemon::start(|_| Reply::Status(409));
    let client = Client::with_timing(&daemon.path, fast_timing());
    assert_eq!(client.state(), Err(Error::Busy));
    assert_eq!(daemon.calls().len(), 1);
}

#[test]
fn watch_liefert_ereignisse_und_ende() {
    let daemon = FakeDaemon::start(|_| {
        Reply::Lines(vec![
            state_json("connecting", "nl.hideservers.net"),
            state_json("connected", "nl.hideservers.net"),
        ])
    });
    let mut watch = Client::with_timing(&daemon.path, fast_timing()).watch().unwrap();

    assert_eq!(watch.next_state().unwrap().unwrap().code, "connecting");
    assert_eq!(watch.next_state().unwrap().unwrap().code, "connected");
    assert_eq!(watch.next_state().unwrap(), None);
}

// --- Observation ---

/// Observes until `count` reports have arrived and returns them. `decide` determines the answer.
fn observe_n<F>(client: &Client, count: usize, mut decide: F) -> Vec<Observation>
where
    F: FnMut(&Observation) -> Flow,
{
    let mut seen = Vec::new();
    client.observe(
        || false,
        |observation| {
            let flow = decide(&observation);
            seen.push(observation);
            if seen.len() >= count { Flow::Stop } else { flow }
        },
    );
    seen
}

fn code(observation: &Observation) -> Option<&str> {
    match observation {
        Observation::State { state, .. } => Some(state.code.as_str()),
        _ => None,
    }
}

#[test]
fn beobachtung_meldet_version_und_anfangszustand() {
    let daemon = FakeDaemon::start(|call| match call {
        "GET /version" => Reply::Text("0.9.14".to_owned()),
        "GET /state" => Reply::Json(result(&state_json("connected", "nl.hideservers.net"))),
        "GET /watch" => Reply::Lines(vec![]),
        _ => Reply::Status(404),
    });
    let seen = observe_n(&Client::with_timing(&daemon.path, fast_timing()), 2, |_| Flow::Continue);

    assert_eq!(seen[0], Observation::Version("0.9.14".to_owned()));
    let Observation::State { state, blocked } = &seen[1] else {
        panic!("{:?}", seen[1])
    };
    assert_eq!(state.code, "connected");
    assert_eq!(state.host.as_deref(), Some("nl.hideservers.net"));
    assert!(!blocked);
    // The observation is opened before `/state`, so that no event gets lost.
    assert_eq!(daemon.calls(), vec!["GET /watch", "GET /version", "GET /state"]);
}

#[test]
fn beobachtung_meldet_ereignisse_in_reihenfolge() {
    let daemon = FakeDaemon::start(|call| match call {
        "GET /version" => Reply::Text("0.9.14".to_owned()),
        "GET /state" => Reply::Json(result(&state_json("clean", ""))),
        "GET /watch" => Reply::Lines(vec![
            state_json("connecting", "nl.hideservers.net"),
            state_json("connected", "nl.hideservers.net"),
        ]),
        _ => Reply::Status(404),
    });
    let seen = observe_n(&Client::with_timing(&daemon.path, fast_timing()), 4, |_| Flow::Continue);
    let codes: Vec<_> = seen.iter().filter_map(code).collect();
    assert_eq!(codes, vec!["clean", "connecting", "connected"]);
}

#[test]
fn routen_werden_bei_jedem_ereignis_neu_geprueft() {
    let daemon = normal_daemon();
    let client = Client::with_timing(&daemon.path, fast_timing());
    let mut checks = 0;
    let mut seen = Vec::new();
    client.observe(
        || {
            checks += 1;
            checks > 1
        },
        |observation| {
            seen.push(observation);
            if seen.len() >= 3 { Flow::Stop } else { Flow::Continue }
        },
    );
    // Initial state without a block, the event after it with a block: every report checks anew.
    assert!(matches!(seen[1], Observation::State { blocked: false, .. }));
    assert!(matches!(seen[2], Observation::State { blocked: true, .. }));
    assert_eq!(checks, 2);
}

#[test]
fn nach_abbruch_von_watch_wird_neu_verbunden() {
    let daemon = normal_daemon();
    // Version, initial state, event, then after the interruption version and state again.
    let seen = observe_n(&Client::with_timing(&daemon.path, fast_timing()), 5, |_| Flow::Continue);

    let codes: Vec<_> = seen.iter().filter_map(code).collect();
    assert_eq!(codes, vec!["clean", "connecting", "clean"]);
    assert_eq!(
        daemon.calls(),
        vec![
            "GET /watch",
            "GET /version",
            "GET /state",
            "GET /watch",
            "GET /version",
            "GET /state"
        ]
    );
}

#[test]
fn unbekannter_code_fuehrt_zur_neuen_abfrage() {
    let daemon = FakeDaemon::start(|call| match call {
        "GET /version" => Reply::Text("0.9.14".to_owned()),
        "GET /state" => Reply::Json(result(&state_json("clean", ""))),
        "GET /watch" => Reply::Lines(vec![state_json("ganz neuer code", "")]),
        _ => Reply::Status(404),
    });
    let seen = observe_n(&Client::with_timing(&daemon.path, fast_timing()), 4, |observation| {
        if code(observation) == Some("ganz neuer code") {
            Flow::Refresh
        } else {
            Flow::Continue
        }
    });
    let codes: Vec<_> = seen.iter().filter_map(code).collect();
    assert_eq!(codes, vec!["clean", "ganz neuer code", "clean"]);
    assert_eq!(
        daemon.calls(),
        vec!["GET /watch", "GET /version", "GET /state", "GET /state"]
    );
}

#[test]
fn gesperrter_state_waehrend_des_aufbaus_wird_als_beschaeftigt_gemeldet() {
    let daemon = FakeDaemon::start(|call| match call {
        "GET /version" => Reply::Text("0.9.14".to_owned()),
        "GET /state" => Reply::Status(409),
        "GET /watch" => Reply::Lines(vec![state_json("connected", "nl.hideservers.net")]),
        _ => Reply::Status(404),
    });
    let seen = observe_n(&Client::with_timing(&daemon.path, fast_timing()), 3, |_| Flow::Continue);
    assert_eq!(seen[1], Observation::Busy { blocked: false });
    // The observation keeps running and reports the end of the connection attempt.
    assert_eq!(code(&seen[2]), Some("connected"));
}

#[test]
fn fehlender_dienst_wird_gemeldet_und_erneut_geprueft() {
    let client = Client::with_timing(unique_path(), fast_timing());
    let seen = observe_n(&client, 3, |_| Flow::Continue);
    for observation in &seen {
        assert_eq!(
            *observation,
            Observation::Failed {
                error: Error::NotRunning,
                blocked: false
            }
        );
    }
}

#[test]
fn fehlende_rechte_werden_bei_der_beobachtung_gemeldet() {
    if !support::permissions_are_enforced() {
        return;
    }
    let path = unique_path();
    let _listener = UnixListener::bind(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
    let seen = observe_n(&Client::with_timing(&path, fast_timing()), 1, |_| Flow::Continue);
    assert_eq!(
        seen[0],
        Observation::Failed {
            error: Error::PermissionDenied,
            blocked: false
        }
    );
}

#[test]
fn gestarteter_dienst_wird_von_selbst_erkannt() {
    let path = unique_path();
    let client = Client::with_timing(&path, fast_timing());
    let mut daemon = None;
    let mut seen = Vec::new();
    client.observe(
        || false,
        |observation| {
            // After the first error report the service "starts" without anybody clicking anything.
            if seen.is_empty() {
                daemon = Some(FakeDaemon::start_at(path.clone(), |call| match call {
                    "GET /version" => Reply::Text("0.9.14".to_owned()),
                    "GET /state" => Reply::Json(result(&state_json("clean", ""))),
                    "GET /watch" => Reply::Lines(vec![]),
                    _ => Reply::Status(404),
                }));
            }
            let stop = matches!(observation, Observation::State { .. });
            seen.push(observation);
            if stop { Flow::Stop } else { Flow::Continue }
        },
    );
    assert_eq!(
        seen[0],
        Observation::Failed {
            error: Error::NotRunning,
            blocked: false
        }
    );
    assert_eq!(code(seen.last().unwrap()), Some("clean"));
}

#[test]
fn neuer_versuch_folgt_innerhalb_von_zwei_sekunden() {
    // Without a "check again" button the regular attempt has to be fast enough.
    assert!(Timing::default().retry < Duration::from_secs(2));
}

#[test]
fn beobachtung_ruft_nur_erlaubte_pfade_auf() {
    let daemon = FakeDaemon::start(|call| match call {
        "GET /version" => Reply::Text("0.9.14".to_owned()),
        "GET /state" => Reply::Json(result(&state_json("clean", ""))),
        "GET /watch" => Reply::Lines(vec![state_json("irgendwas", ""), state_json("routed", "")]),
        _ => Reply::Status(404),
    });
    let seen = observe_n(&Client::with_timing(&daemon.path, fast_timing()), 8, |observation| {
        if code(observation) == Some("irgendwas") {
            Flow::Refresh
        } else {
            Flow::Continue
        }
    });
    for observation in &seen {
        assert!(!format!("{observation:?}").contains(FAKE_SECRET));
    }
    for call in daemon.calls() {
        assert!(ALLOWED.contains(&call.as_str()), "nicht erlaubt: {call}");
    }
}

// --- Connecting ---

/// Service where `/connect` answers with `connect_reply` and `/destroy` with `destroy_reply`.
fn connect_daemon(connect_reply: fn() -> Reply, destroy_reply: fn() -> Reply) -> FakeDaemon {
    FakeDaemon::start(move |call| match call {
        "POST /configuration" => Reply::Json(r#"{"result":true}"#.to_owned()),
        "GET /connect" => connect_reply(),
        "GET /destroy" => destroy_reply(),
        _ => Reply::Status(404),
    })
}

fn connected() -> Reply {
    Reply::Json(result(&state_json("connected", "nl.hideservers.net")))
}

fn connect_error() -> Reply {
    Reply::Json(r#"{"error":{"code":"connect","message":"dial failed"}}"#.to_owned())
}

fn clean() -> Reply {
    Reply::Json(result(&state_json("clean", "")))
}

fn assert_no_server_list(daemon: &FakeDaemon) {
    for call in daemon.calls() {
        assert!(ALLOWED.contains(&call.as_str()), "nicht erlaubt: {call}");
        assert!(!call.contains("serverList"), "{call}");
    }
}

#[test]
fn verbinden_setzt_host_und_verbindet() {
    let daemon = connect_daemon(connected, clean);
    let client = Client::with_timing(&daemon.path, fast_timing());
    assert_eq!(
        connect_default(&client, "nl.hideservers.net", true),
        ConnectResult::Connected
    );
    assert_eq!(daemon.calls(), vec!["POST /configuration", "GET /connect"]);
    assert_eq!(daemon.bodies()[0], r#"{"Rest":{"Host":"nl.hideservers.net"}}"#);
    assert_no_server_list(&daemon);
}

#[test]
fn fehlschlag_aus_freiem_zustand_gibt_das_internet_frei() {
    // First case: the internet was free, so `/destroy` follows.
    let daemon = connect_daemon(connect_error, clean);
    let client = Client::with_timing(&daemon.path, fast_timing());
    assert_eq!(
        connect_default(&client, "nl.hideservers.net", true),
        ConnectResult::FailedReleased(Error::Service("dial failed".to_owned()))
    );
    assert_eq!(
        daemon.calls(),
        vec!["POST /configuration", "GET /connect", "GET /destroy"]
    );
    assert_no_server_list(&daemon);
}

#[test]
fn fehlschlag_aus_gesperrtem_zustand_laesst_die_sperre_bestehen() {
    // Second case: the internet was blocked, so no `/destroy`.
    let daemon = connect_daemon(connect_error, clean);
    let client = Client::with_timing(&daemon.path, fast_timing());
    assert!(matches!(
        connect_default(&client, "nl.hideservers.net", false),
        ConnectResult::FailedBlocked(_)
    ));
    assert_eq!(daemon.calls(), vec!["POST /configuration", "GET /connect"]);
}

#[test]
fn anderer_endzustand_als_connected_gilt_als_fehlschlag() {
    let daemon = connect_daemon(|| Reply::Json(result(&state_json("routed", ""))), clean);
    let client = Client::with_timing(&daemon.path, fast_timing());
    assert!(matches!(
        connect_default(&client, "nl.hideservers.net", true),
        ConnectResult::FailedReleased(_)
    ));
    assert!(daemon.calls().contains(&"GET /destroy".to_owned()));
}

#[test]
fn scheitert_auch_destroy_wird_der_notausgang_gemeldet() {
    let daemon = connect_daemon(connect_error, || Reply::Status(500));
    let client = Client::with_timing(&daemon.path, fast_timing());
    assert!(matches!(
        connect_default(&client, "nl.hideservers.net", true),
        ConnectResult::ReleaseFailed(_)
    ));
}

#[test]
fn beschaeftigter_dienst_startet_keinen_aufbau_und_gibt_nichts_frei() {
    let daemon = connect_daemon(|| Reply::Status(409), clean);
    let client = Client::with_timing(&daemon.path, fast_timing());
    assert_eq!(
        connect_default(&client, "nl.hideservers.net", true),
        ConnectResult::NotStarted(Error::Busy)
    );
    assert!(!daemon.calls().contains(&"GET /destroy".to_owned()));
}

#[test]
fn ungueltiger_server_erreicht_den_dienst_nicht() {
    let daemon = connect_daemon(connected, clean);
    let client = Client::with_timing(&daemon.path, fast_timing());
    assert_eq!(
        connect_default(&client, "boese.example.org", true),
        ConnectResult::NotStarted(Error::InvalidHost)
    );
    assert_eq!(daemon.calls(), [] as [std::string::String; 0]);
}

#[test]
fn fehler_beim_setzen_des_hosts_startet_keinen_aufbau() {
    let daemon = FakeDaemon::start(|_| Reply::Status(500));
    let client = Client::with_timing(&daemon.path, fast_timing());
    assert_eq!(
        connect_default(&client, "nl.hideservers.net", true),
        ConnectResult::NotStarted(Error::Status(500))
    );
    assert_eq!(daemon.calls(), vec!["POST /configuration"]);
}

// --- Disconnecting ---

#[test]
fn trennen_nutzt_destroy_und_nie_disconnect() {
    let daemon = normal_daemon();
    let client = Client::with_timing(&daemon.path, fast_timing());
    let mut monitor = Monitor::new();
    monitor.observe(Observation::State {
        state: client.state().unwrap(),
        blocked: false,
    });
    monitor.observe(Observation::State {
        state: client.connect().unwrap(),
        blocked: false,
    });

    assert_eq!(monitor.begin_disconnect(false), DisconnectStart::Now);
    let result = run_disconnect(&client);
    assert_eq!(result, Ok(()));
    assert!(!monitor.finish_disconnect(&result));
    monitor.observe(Observation::State {
        state: client.state().unwrap(),
        blocked: false,
    });

    assert_eq!(
        celata::i18n::text(celata::i18n::Language::De, monitor.view().message.title),
        "Nicht verbunden"
    );
    let calls = daemon.calls();
    assert!(calls.contains(&"GET /destroy".to_owned()));
    assert!(!calls.iter().any(|call| call.contains("disconnect")), "{calls:?}");
    for call in &calls {
        assert!(ALLOWED.contains(&call.as_str()), "nicht erlaubt: {call}");
    }
}

#[test]
fn trennen_ohne_dienst_meldet_den_fehler() {
    let client = Client::with_timing(unique_path(), fast_timing());
    assert_eq!(run_disconnect(&client), Err(Error::NotRunning));
}

// --- Unblocking the internet ---

/// A monitor that, as with an existing block, currently sees `routed` with blocking routes.
fn blocked_monitor(client: &Client) -> Monitor {
    let mut monitor = Monitor::new();
    let mut state = client.state().unwrap();
    "routed".clone_into(&mut state.code);
    monitor.observe(Observation::State { state, blocked: true });
    assert_eq!(monitor.display(), Some(&DisplayState::Blocked));
    monitor
}

/// Re-enacts the sequence from the user interface: `/destroy` only if `confirm_unblock` agrees.
fn unblock_if_confirmed(client: &Client, monitor: &mut Monitor) {
    if monitor.confirm_unblock() {
        let result = run_unblock(client);
        monitor.finish_unblock(&result);
    }
}

#[test]
fn ohne_bestaetigung_wird_die_sperre_nicht_aufgehoben() {
    let daemon = normal_daemon();
    let client = Client::with_timing(&daemon.path, fast_timing());
    let mut monitor = blocked_monitor(&client);

    unblock_if_confirmed(&client, &mut monitor);
    monitor.ask_unblock();
    monitor.cancel_unblock();
    unblock_if_confirmed(&client, &mut monitor);

    assert!(
        !daemon.calls().contains(&"GET /destroy".to_owned()),
        "{:?}",
        daemon.calls()
    );
}

#[test]
fn nach_bestaetigung_folgt_destroy_und_nicht_verbunden() {
    let daemon = normal_daemon();
    let client = Client::with_timing(&daemon.path, fast_timing());
    let mut monitor = blocked_monitor(&client);

    assert!(monitor.ask_unblock());
    unblock_if_confirmed(&client, &mut monitor);
    monitor.observe(Observation::State {
        state: client.state().unwrap(),
        blocked: false,
    });

    assert_eq!(monitor.display(), Some(&DisplayState::NotConnected));
    let calls = daemon.calls();
    assert_eq!(calls.iter().filter(|call| *call == "GET /destroy").count(), 1);
    assert!(!calls.iter().any(|call| call.contains("disconnect")));
}

#[test]
fn scheitert_destroy_erscheint_der_notausgang() {
    let daemon = FakeDaemon::start(|call| match call {
        "GET /state" => Reply::Json(result(&state_json("routed", ""))),
        _ => Reply::Status(409),
    });
    let client = Client::with_timing(&daemon.path, fast_timing());
    let mut monitor = blocked_monitor(&client);

    monitor.ask_unblock();
    unblock_if_confirmed(&client, &mut monitor);
    monitor.observe(Observation::State {
        state: client.state().unwrap(),
        blocked: true,
    });

    let view = monitor.view();
    assert_eq!(view.message.command, Some(STOP_COMMAND));
    assert_eq!(view.notice.unwrap().command, Some(STOP_COMMAND));
    // `/destroy` was repeated up to the limit.
    assert!(daemon.calls().iter().filter(|call| *call == "GET /destroy").count() > 1);
}

#[test]
fn dienst_nicht_erreichbar_und_internet_gesperrt() {
    let client = Client::with_timing(unique_path(), fast_timing());
    let mut monitor = Monitor::new();
    client.observe(
        || true,
        |observation| {
            monitor.observe(observation);
            Flow::Stop
        },
    );
    assert_eq!(monitor.display(), Some(&DisplayState::ServiceUnreachableBlocked));
    let view = monitor.view();
    assert!(view.message.warning && !view.can_unblock);
    assert_eq!(view.message.command, Some(STOP_COMMAND));
}

#[test]
fn wiederverbindung_mit_sperre_wird_angezeigt() {
    let daemon = FakeDaemon::start(|call| match call {
        "GET /version" => Reply::Text("0.9.14".to_owned()),
        "GET /state" => Reply::Json(result(&state_json("dpd timeout", ""))),
        "GET /watch" => Reply::Lines(vec![state_json("connecting", "nl.hideservers.net")]),
        _ => Reply::Status(404),
    });
    let client = Client::with_timing(&daemon.path, fast_timing());
    let mut monitor = Monitor::new();
    let mut displays = Vec::new();
    client.observe(
        || true,
        |observation| {
            let (flow, _) = monitor.observe(observation);
            displays.push(monitor.display().cloned());
            if displays.len() >= 3 { Flow::Stop } else { flow }
        },
    );
    // First the block after the interruption, then the automatic reconnection, still with the block.
    assert_eq!(displays[1], Some(DisplayState::Blocked));
    assert_eq!(displays[2], Some(DisplayState::ConnectingBlocked));
    let view = monitor.view();
    assert!(view.message.warning && view.can_unblock);
}

// --- Switching servers ---

const FROM: &str = "nl.hideservers.net";
const TO: &str = "de.hideservers.net";

/// Service for a switch: `/disconnect` with `disconnect_reply`, the n-th `/connect` (from 0) with
/// `connect_reply(n)`, `/destroy` succeeds.
fn switch_daemon(disconnect_reply: fn() -> Reply, connect_reply: fn(u32) -> Reply) -> FakeDaemon {
    let connects = Arc::new(AtomicU32::new(0));
    FakeDaemon::start(move |call| match call {
        "GET /disconnect" => disconnect_reply(),
        "POST /configuration" => Reply::Json(r#"{"result":true}"#.to_owned()),
        "GET /connect" => connect_reply(connects.fetch_add(1, Ordering::SeqCst)),
        "GET /destroy" => Reply::Json(result(&state_json("clean", ""))),
        _ => Reply::Status(404),
    })
}

fn routed() -> Reply {
    Reply::Json(result(&state_json("routed", FROM)))
}

fn connected_to_de(_: u32) -> Reply {
    Reply::Json(result(&state_json("connected", TO)))
}

fn always_fails(_: u32) -> Reply {
    connect_error()
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "has the signature of the check it stands in for"
)]
fn armed() -> std::io::Result<bool> {
    Ok(true)
}

fn switch_with(daemon: &FakeDaemon, armed: fn() -> std::io::Result<bool>, resolves: bool) -> SwitchResult {
    let client = Client::with_timing(&daemon.path, fast_timing());
    run_switch(&client, FROM, TO, armed, |_| resolves, |_| true)
}

fn assert_allowed(daemon: &FakeDaemon) {
    for call in daemon.calls() {
        assert!(ALLOWED.contains(&call.as_str()), "nicht erlaubt: {call}");
    }
}

#[test]
fn wechsel_ruft_trennen_host_und_verbinden_in_dieser_reihenfolge() {
    // A successful switch makes exactly these calls, in this order
    let daemon = switch_daemon(routed, connected_to_de);
    assert_eq!(switch_with(&daemon, armed, true), SwitchResult::Switched);
    assert_eq!(
        daemon.calls(),
        vec!["GET /disconnect", "POST /configuration", "GET /connect"]
    );
    assert_eq!(daemon.bodies()[1], r#"{"Rest":{"Host":"de.hideservers.net"}}"#);
    assert_allowed(&daemon);
}

#[test]
fn ohne_bereite_sperre_wird_der_dienst_nicht_aufgerufen() {
    // Block not ready: no call at all
    let daemon = switch_daemon(routed, connected_to_de);
    assert_eq!(switch_with(&daemon, || Ok(false), true), SwitchResult::Refused);
    assert_eq!(
        switch_with(&daemon, || Err(std::io::ErrorKind::NotFound.into()), true),
        SwitchResult::Refused
    );
    assert_eq!(daemon.calls(), [] as [std::string::String; 0]);
}

#[test]
fn name_ohne_adresse_ruft_den_dienst_nicht_auf() {
    // New name without an address: no call at all
    let daemon = switch_daemon(routed, connected_to_de);
    let client = Client::with_timing(&daemon.path, fast_timing());
    let asked = std::cell::RefCell::new(Vec::new());
    let result = run_switch(
        &client,
        FROM,
        TO,
        armed,
        |host| {
            asked.borrow_mut().push(host.to_owned());
            false
        },
        |_| true,
    );
    assert_eq!(result, SwitchResult::Unreachable);
    assert_eq!(asked.into_inner(), vec![TO.to_owned()], "geprüft wird der neue Name");
    assert_eq!(daemon.calls(), [] as [std::string::String; 0]);
}

#[test]
fn beschaeftigter_dienst_beim_trennen_beendet_den_wechsel() {
    // HTTP 409 on `/disconnect` ends the switch without a further call
    let daemon = switch_daemon(|| Reply::Status(409), connected_to_de);
    assert_eq!(switch_with(&daemon, armed, true), SwitchResult::NotStarted(Error::Busy));
    assert_eq!(daemon.calls(), vec!["GET /disconnect"]);
}

#[test]
fn fehler_beim_trennen_gilt_als_unklar() {
    let daemon = switch_daemon(connect_error, connected_to_de);
    assert_eq!(
        switch_with(&daemon, armed, true),
        SwitchResult::Unclear(Error::Service("dial failed".to_owned()))
    );
    assert_eq!(daemon.calls(), vec!["GET /disconnect"]);

    let daemon = switch_daemon(|| Reply::Json(result(&state_json("connected", FROM))), connected_to_de);
    assert_eq!(
        switch_with(&daemon, armed, true),
        SwitchResult::Unclear(Error::InvalidResponse)
    );
    assert_eq!(daemon.calls(), vec!["GET /disconnect"]);
}

#[test]
fn trennen_waehrend_des_wechsels_fuehrt_zu_destroy_und_keinem_connect() {
    // A requested disconnect leads to `/destroy` and to no further `/connect`
    let daemon = switch_daemon(routed, connected_to_de);
    let client = Client::with_timing(&daemon.path, fast_timing());
    let mut asked = 0;
    let result = run_switch(
        &client,
        FROM,
        TO,
        armed,
        |_| true,
        |phase| {
            assert_eq!(phase, Phase::Forward);
            asked += 1;
            asked == 1
        },
    );
    assert_eq!(result, SwitchResult::Stopped(Ok(())));
    assert_eq!(daemon.calls(), vec!["GET /disconnect", "GET /destroy"]);
}

fn first_fails(attempt: u32) -> Reply {
    if attempt == 0 {
        connect_error()
    } else {
        Reply::Json(result(&state_json("connected", FROM)))
    }
}

#[test]
fn gescheiterter_neuer_server_fuehrt_zurueck_zum_alten() {
    // If `/connect` fails, the old host is configured and connected again
    let daemon = switch_daemon(routed, first_fails);
    assert_eq!(
        switch_with(&daemon, armed, true),
        SwitchResult::Returned(Error::Service("dial failed".to_owned()))
    );
    assert_eq!(
        daemon.calls(),
        vec![
            "GET /disconnect",
            "POST /configuration",
            "GET /connect",
            "POST /configuration",
            "GET /connect"
        ]
    );
    assert_eq!(daemon.bodies()[3], r#"{"Rest":{"Host":"nl.hideservers.net"}}"#);
    assert_allowed(&daemon);
}

#[test]
fn gescheiterter_rueckweg_laesst_die_sperre_bestehen() {
    // If the way back fails as well: no `/destroy` on its own
    let daemon = switch_daemon(routed, always_fails);
    assert!(matches!(switch_with(&daemon, armed, true), SwitchResult::Stranded(_)));
    assert_eq!(daemon.calls().iter().filter(|call| *call == "GET /connect").count(), 2);
    assert!(!daemon.calls().contains(&"GET /destroy".to_owned()));
    assert_allowed(&daemon);
}

#[test]
fn trennen_vor_dem_rueckweg_fuehrt_zu_destroy() {
    let daemon = switch_daemon(routed, always_fails);
    let client = Client::with_timing(&daemon.path, fast_timing());
    let result = run_switch(&client, FROM, TO, armed, |_| true, |phase| phase == Phase::Forward);
    assert_eq!(result, SwitchResult::Stopped(Ok(())));
    assert_eq!(
        daemon.calls(),
        vec!["GET /disconnect", "POST /configuration", "GET /connect", "GET /destroy"]
    );
}

#[test]
fn hinweise_zum_wechsel_enthalten_keine_schluessel() {
    // No invented key values in notices
    for result in [
        SwitchResult::Switched,
        SwitchResult::Returned(Error::Timeout),
        SwitchResult::Stranded(Error::Timeout),
        SwitchResult::Refused,
        SwitchResult::Unreachable,
        SwitchResult::NotStarted(Error::Busy),
        SwitchResult::Unclear(Error::Timeout),
        SwitchResult::Stopped(Ok(())),
    ] {
        if let Some(notice) = result.notice(FROM, TO) {
            for language in celata::i18n::Language::ALL {
                assert!(!notice.text.render(language).contains(FAKE_SECRET), "{result:?}");
            }
            assert_eq!(notice.command, None);
        }
    }
    // The imitated service delivers invented keys in the state. They must not end up in the notice.
    let daemon = switch_daemon(routed, always_fails);
    let failed = switch_with(&daemon, armed, true);
    for language in celata::i18n::Language::ALL {
        assert!(
            !failed
                .notice(FROM, TO)
                .unwrap()
                .text
                .render(language)
                .contains(FAKE_SECRET)
        );
    }
}

/// An error text of the service is cleaned and shortened on every path, also with HTTP 200.
#[test]
fn fehlertext_des_dienstes_wird_auch_bei_http_200_bereinigt() {
    let long = "x".repeat(5000);
    let daemon = FakeDaemon::start(move |call| match call {
        "GET /connect" => Reply::Json(format!(
            r#"{{"error":{{"code":"x","message":"line one\nline two\u001b[31m {long}"}}}}"#
        )),
        _ => Reply::Status(404),
    });
    let client = Client::with_timing(&daemon.path, fast_timing());
    let Err(Error::Service(message)) = client.connect() else {
        panic!("expected a service error")
    };
    assert!(message.starts_with("line oneline two[31m xxx"), "{message}");
    assert_eq!(message.chars().count(), 300);
    assert!(!message.chars().any(char::is_control));
}
