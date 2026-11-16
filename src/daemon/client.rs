// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! The calls to the service that Celata is allowed to make.
//!
//! The client deliberately offers no general function for other paths. That way forbidden calls such as
//! `/serverList` (it overwrites the configuration of the service, issue eventure/hide.client.linux#61) or
//! `/token` cannot be triggered at all.

use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;

use super::http::{self, Error, LineStream, Method};
use crate::servers::is_valid_host;
use crate::settings::{self, Settings};

/// Path of the control socket.
pub const DEFAULT_SOCKET: &str = "/run/hide.me/control.sock";

/// Time limits of the calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timing {
    pub call: Duration,
    pub connect: Duration,
    pub destroy_total: Duration,
    pub destroy_pause: Duration,
    /// Waiting time before the observation reconnects after an error or a lost connection. It is short
    /// enough for a started service to be recognized within 2 seconds.
    pub retry: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            call: Duration::from_secs(5),
            connect: Duration::from_secs(100),
            destroy_total: Duration::from_secs(100),
            destroy_pause: Duration::from_secs(1),
            retry: Duration::from_secs(1),
        }
    }
}

/// State of the service. Only these fields are evaluated. Keys such as `publicKey`, `presharedKey` and
/// `sessionToken` and details such as `endpoint` and `gateway` are deliberately not declared: they never
/// end up in this type and therefore in no output.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct ServiceState {
    pub code: String,
    #[serde(default)]
    pub host: Option<String>,
    /// Bytes received (only while `connected`).
    #[serde(default)]
    pub rx: Option<u64>,
    /// Bytes sent (only while `connected`).
    #[serde(default)]
    pub tx: Option<u64>,
    /// Time of the transition into the current state (RFC 3339).
    #[serde(default, rename = "timestamp")]
    pub since: Option<String>,
    /// Addresses inside the tunnel.
    #[serde(default, rename = "allowedIps")]
    pub addresses: Vec<String>,
    /// DNS servers inside the tunnel.
    #[serde(default, rename = "DNS")]
    pub dns: Vec<String>,
    /// Result of `/externalIps`; it arrives through `/watch` with the code `external ips`.
    #[serde(default, rename = "external_ip")]
    pub external_v4: Option<String>,
    #[serde(default, rename = "external_ipv6")]
    pub external_v6: Option<String>,
}

#[derive(Deserialize)]
struct Envelope<T> {
    result: Option<T>,
    error: Option<ServiceFailure>,
}

#[derive(Deserialize)]
struct ServiceFailure {
    message: String,
}

fn parse_envelope<'a, T: Deserialize<'a>>(content: &'a [u8]) -> Result<T, Error> {
    let envelope: Envelope<T> = serde_json::from_slice(content).map_err(|_| Error::InvalidResponse)?;
    if let Some(failure) = envelope.error {
        return Err(http::service_error(&failure.message));
    }
    envelope.result.ok_or(Error::InvalidResponse)
}

/// Connection to the hide.me service.
#[derive(Debug, Clone)]
pub struct Client {
    socket: PathBuf,
    timing: Timing,
}

impl Default for Client {
    fn default() -> Self {
        Self::new(DEFAULT_SOCKET)
    }
}

impl Client {
    pub fn new(socket: impl AsRef<Path>) -> Self {
        Self::with_timing(socket, Timing::default())
    }

    pub fn with_timing(socket: impl AsRef<Path>, timing: Timing) -> Self {
        Self {
            socket: socket.as_ref().to_path_buf(),
            timing,
        }
    }

    /// Asks for the version of the service.
    pub fn version(&self) -> Result<String, Error> {
        let content = http::request(&self.socket, Method::Get, "/version", None, self.timing.call)?;
        String::from_utf8(content)
            .map(|text| text.trim().to_owned())
            .map_err(|_| Error::InvalidResponse)
    }

    /// Reads the current state.
    pub fn state(&self) -> Result<ServiceState, Error> {
        let content = http::request(&self.socket, Method::Get, "/state", None, self.timing.call)?;
        parse_envelope(&content)
    }

    /// Opens the continuous observation of state changes.
    pub fn watch(&self) -> Result<Watch, Error> {
        Ok(Watch {
            lines: LineStream::open(&self.socket, "/watch")?,
        })
    }

    /// Sets server and settings for the next connection (`POST /configuration`).
    ///
    /// With `full == false` only `{"Rest":{"Host":…}}` is sent, otherwise the complete set from
    /// [`settings::to_service_json`]. If the service refuses (HTTP 400), its error text comes back as
    /// [`Error::Service`].
    pub fn configure(&self, settings: &Settings, host: &str, full: bool) -> Result<(), Error> {
        if !is_valid_host(host) {
            return Err(Error::InvalidHost);
        }
        let body = if full {
            settings::to_service_json(settings, host)
        } else {
            serde_json::json!({ "Rest": { "Host": host } }).to_string()
        };
        self.post_configuration(&body)
    }

    /// Sends the complete set of settings without a host (`POST /configuration`), before `/route`.
    pub fn configure_without_host(&self, settings: &Settings) -> Result<(), Error> {
        self.post_configuration(&settings::to_service_json_without_host(settings))
    }

    /// `POST /configuration` with a finished body; the service must confirm with `true`.
    fn post_configuration(&self, body: &str) -> Result<(), Error> {
        let content = http::request(
            &self.socket,
            Method::Post,
            "/configuration",
            Some(body.as_bytes()),
            self.timing.call,
        )?;
        if parse_envelope::<bool>(&content)? {
            Ok(())
        } else {
            Err(Error::InvalidResponse)
        }
    }

    /// Asks the service to determine the public addresses (`GET /externalIps`). The result arrives through
    /// `/watch` as `external ips`. Never while the internet is blocked (`Monitor::begin_external_ip`).
    pub fn external_ips(&self) -> Result<(), Error> {
        http::request(&self.socket, Method::Get, "/externalIps", None, self.timing.call).map(|_| ())
    }

    /// Builds the routes from `clean`, with the kill switch turned on the blocking routes (`GET /route`).
    /// Only when the user presses "Block internet".
    pub fn route(&self) -> Result<ServiceState, Error> {
        let content = http::request(&self.socket, Method::Get, "/route", None, self.timing.call)?;
        parse_envelope(&content)
    }

    /// Connects. The call answers only when the connection is established.
    pub fn connect(&self) -> Result<ServiceState, Error> {
        let content = http::request(&self.socket, Method::Get, "/connect", None, self.timing.connect)?;
        parse_envelope(&content)
    }

    /// Disconnects but leaves the block in place: the service goes to `routed`.
    ///
    /// Use it only within a switch to another server. "Disconnect" and "Unblock internet" keep using
    /// [`Client::destroy`]. The time limit is that of `/connect`, because the service signs off from the old
    /// server here. On HTTP 409 the call is not repeated.
    pub fn disconnect(&self) -> Result<ServiceState, Error> {
        let content = http::request(&self.socket, Method::Get, "/disconnect", None, self.timing.connect)?;
        parse_envelope(&content)
    }

    /// Disconnects and lifts the block. As long as the service is busy, the call is repeated.
    pub fn destroy(&self) -> Result<ServiceState, Error> {
        let deadline = Instant::now() + self.timing.destroy_total;
        loop {
            let attempt = http::request(&self.socket, Method::Get, "/destroy", None, self.timing.call)
                .and_then(|content| parse_envelope(&content));
            match attempt {
                Err(Error::Busy | Error::Timeout) if Instant::now() < deadline => {
                    thread::sleep(self.timing.destroy_pause);
                }
                other => return other,
            }
        }
    }
}

/// Report of the observation to the program logic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Observation {
    /// The service is reachable and reports this version. Comes after every reconnection.
    Version(String),
    /// State from `/state` or `/watch`, together with the result of the route check.
    State { state: ServiceState, blocked: bool },
    /// `/state` was locked because a connection was being set up or torn down (HTTP 409).
    Busy { blocked: bool },
    /// The service is not reachable, or access fails.
    Failed { error: Error, blocked: bool },
}

/// Answer of the program logic to a report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Continue,
    /// Ask `/state` again, for example after an unknown code.
    Refresh,
    /// Stop observing.
    Stop,
}

impl Client {
    /// Observes the service until `sink` returns [`Flow::Stop`].
    ///
    /// Sequence per connection: open `/watch`, ask `/version` and once `/state`, then read the events of
    /// `/watch`. The observation is opened before `/state`, so that no event gets lost in between. With every
    /// report the routes are checked again with `blocked`. If `/watch` breaks off or the service is not
    /// reachable, the observation reconnects by itself after [`Timing::retry`].
    pub fn observe<B, S>(&self, mut blocked: B, mut sink: S)
    where
        B: FnMut() -> bool,
        S: FnMut(Observation) -> Flow,
    {
        while self.observe_connection(&mut blocked, &mut sink) != Flow::Stop {
            thread::sleep(self.timing.retry);
        }
    }

    /// One connection of the observation. Returns [`Flow::Stop`] or [`Flow::Continue`] (reconnect).
    fn observe_connection<B, S>(&self, blocked: &mut B, sink: &mut S) -> Flow
    where
        B: FnMut() -> bool,
        S: FnMut(Observation) -> Flow,
    {
        let failed = |error: Error, blocked: &mut B, sink: &mut S| match sink(Observation::Failed {
            error,
            blocked: blocked(),
        }) {
            Flow::Stop => Flow::Stop,
            _ => Flow::Continue,
        };
        let mut watch = match self.watch() {
            Ok(watch) => watch,
            Err(error) => return failed(error, blocked, sink),
        };
        match self.version() {
            Ok(version) => {
                if sink(Observation::Version(version)) == Flow::Stop {
                    return Flow::Stop;
                }
            }
            Err(error) => return failed(error, blocked, sink),
        }
        if let Err(flow) = self.observe_state(blocked, sink) {
            return flow;
        }
        loop {
            // Broken off or unreadable report: reconnect and ask `/state`.
            let Ok(Some(state)) = watch.next_state() else {
                return Flow::Continue;
            };
            match sink(Observation::State {
                state,
                blocked: blocked(),
            }) {
                Flow::Stop => return Flow::Stop,
                Flow::Refresh => {
                    if let Err(flow) = self.observe_state(blocked, sink) {
                        return flow;
                    }
                }
                Flow::Continue => {}
            }
        }
    }

    /// Asks `/state` and reports the result. `Err` means: end the connection with this result.
    fn observe_state<B, S>(&self, blocked: &mut B, sink: &mut S) -> Result<(), Flow>
    where
        B: FnMut() -> bool,
        S: FnMut(Observation) -> Flow,
    {
        let (observation, keep_going) = match self.state() {
            Ok(state) => (
                Observation::State {
                    state,
                    blocked: blocked(),
                },
                true,
            ),
            Err(Error::Busy) => (Observation::Busy { blocked: blocked() }, true),
            Err(error) => (
                Observation::Failed {
                    error,
                    blocked: blocked(),
                },
                false,
            ),
        };
        match sink(observation) {
            Flow::Stop => Err(Flow::Stop),
            _ if keep_going => Ok(()),
            _ => Err(Flow::Continue),
        }
    }
}

/// Running observation of the service.
pub struct Watch {
    lines: LineStream,
}

impl Watch {
    /// Returns the next event, or `None` if the service has closed the connection.
    pub fn next_state(&mut self) -> Result<Option<ServiceState>, Error> {
        match self.lines.next_line()? {
            None => Ok(None),
            Some(line) => serde_json::from_slice(&line)
                .map(Some)
                .map_err(|_| Error::InvalidResponse),
        }
    }
}
