// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! Imitated hide.me service on a test socket. All key values are invented.
//!
//! Not every test file uses all helpers, which is why `dead_code` is allowed here.
#![allow(dead_code)]

use std::fmt::Write as _;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

/// Invented value: it must not show up in any output of Celata.
pub const FAKE_SECRET: &str = "ERFUNDENER-TESTWERT-KEIN-ECHTER-SCHLUESSEL";

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A unique test path that does not exist yet.
pub fn unique_path() -> PathBuf {
    let number = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("celata-test-{}-{number}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    dir.join("control.sock")
}

/// Do file permissions stop this process? Not if it runs as root, as it may in a container. Tests that take a
/// permission away to provoke "permission denied" cannot prove anything then and end early.
pub fn permissions_are_enforced() -> bool {
    use std::os::unix::fs::PermissionsExt;
    let probe = unique_path().with_file_name("no-access");
    fs::write(&probe, b"x").unwrap();
    fs::set_permissions(&probe, fs::Permissions::from_mode(0o000)).unwrap();
    let enforced = fs::File::open(&probe).is_err();
    if !enforced {
        eprintln!("note: running with privileges that ignore file permissions; this check is skipped");
    }
    enforced
}

/// State in the format of the service, including invented keys.
pub fn state_json(code: &str, host: &str) -> String {
    format!(
        r#"{{"code":"{code}","timestamp":"2026-11-06T12:00:00Z","publicKey":"{FAKE_SECRET}","presharedKey":"{FAKE_SECRET}","sessionToken":"{FAKE_SECRET}","rx":1,"tx":2,"host":"{host}"}}"#
    )
}

/// Invented key for the tests of the settings.
pub const SECRET_005: &str = "GEHEIM-SCHLUESSEL-123";

/// State `connected` with all detail fields as `/state` delivers them, and invented keys.
pub fn connected_json_with_details(host: &str) -> String {
    format!(
        r#"{{"code":"connected","host":"{host}","timestamp":"2026-11-09T12:34:56.123456789+01:00","rx":12345678,"tx":2345678,"allowedIps":["10.128.0.2","fd00::2"],"DNS":["10.128.0.1"],"gateway":"10.128.0.1","endpoint":"203.0.113.7:432","persistentKeepalive":20,"publicKey":"{SECRET_005}","presharedKey":"{SECRET_005}","sessionToken":"{SECRET_005}"}}"#
    )
}

/// `/watch` report with the result of `/externalIps`.
pub fn external_ips_json(v4: &str, v6: &str) -> String {
    format!(r#"{{"code":"external ips","external_ip":"{v4}","external_ipv6":"{v6}"}}"#)
}

pub fn result(state: &str) -> String {
    format!(r#"{{"result":{state}}}"#)
}

pub enum Reply {
    Json(String),
    Text(String),
    Status(u16),
    /// HTTP error with JSON content, for example HTTP 400 with `{"error":{…}}`.
    StatusJson(u16, String),
    /// Response in `chunked` encoding with one line per chunk.
    Lines(Vec<String>),
}

#[derive(Clone)]
pub struct FakeDaemon {
    pub path: PathBuf,
    calls: Arc<Mutex<Vec<String>>>,
    bodies: Arc<Mutex<Vec<String>>>,
}

impl FakeDaemon {
    /// Starts the service. `handler` receives the call as "GET /state".
    pub fn start<F>(handler: F) -> Self
    where
        F: Fn(&str) -> Reply + Send + 'static,
    {
        Self::start_at(unique_path(), handler)
    }

    /// Starts the service on a given socket path.
    pub fn start_at<F>(path: PathBuf, handler: F) -> Self
    where
        F: Fn(&str) -> Reply + Send + 'static,
    {
        let listener = UnixListener::bind(&path).unwrap();
        let daemon = Self {
            path,
            calls: Arc::new(Mutex::new(Vec::new())),
            bodies: Arc::new(Mutex::new(Vec::new())),
        };
        let shared = daemon.clone();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { return };
                shared.serve(stream, &handler);
            }
        });
        daemon
    }

    fn serve<F: Fn(&str) -> Reply>(&self, mut stream: UnixStream, handler: &F) {
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut first = String::new();
        reader.read_line(&mut first).unwrap();
        let mut parts = first.split_whitespace();
        let call = format!("{} {}", parts.next().unwrap_or(""), parts.next().unwrap_or(""));

        let mut length = 0;
        loop {
            let mut header = String::new();
            reader.read_line(&mut header).unwrap();
            if header.trim_end().is_empty() {
                break;
            }
            if let Some(value) = header.to_ascii_lowercase().strip_prefix("content-length:") {
                length = value.trim().parse().unwrap();
            }
        }
        let mut body = vec![0; length];
        reader.read_exact(&mut body).unwrap();

        self.calls.lock().unwrap().push(call.clone());
        self.bodies.lock().unwrap().push(String::from_utf8(body).unwrap());

        let response = match handler(&call) {
            Reply::Json(content) | Reply::Text(content) => {
                format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{content}", content.len())
            }
            Reply::Status(code) => format!("HTTP/1.1 {code} Fehler\r\nContent-Length: 0\r\n\r\n"),
            Reply::StatusJson(code, content) => {
                format!(
                    "HTTP/1.1 {code} Fehler\r\nContent-Length: {}\r\n\r\n{content}",
                    content.len()
                )
            }
            Reply::Lines(lines) => {
                let mut text = String::from("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n");
                for line in lines {
                    let line = format!("{line}\n");
                    let _ = write!(text, "{:x}\r\n{line}\r\n", line.len());
                }
                text.push_str("0\r\n\r\n");
                text
            }
        };
        let _ = stream.write_all(response.as_bytes());
    }

    pub fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    pub fn bodies(&self) -> Vec<String> {
        self.bodies.lock().unwrap().clone()
    }
}
