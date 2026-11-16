// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! Recognizes the internet block from the routes of the system.
//!
//! The service does not report the block itself. To block, it adds default routes to the loopback
//! interface in its own routing table (`LoopbackRoutesAdd` in hide.me v0.9.14). Reading the table needs no
//! administrator rights.
//!
//! The expected output was confirmed against the real service (2026-11-08, hide.me v0.9.14).

use std::io;

use serde::Deserialize;

const ROUTING_TABLE: &str = "55555";
const IP_COMMANDS: [&str; 2] = ["/usr/sbin/ip", "/usr/bin/ip"];
/// Largest output of `ip` that is read. The table of the service has only a few entries.
const MAX_OUTPUT: u64 = 1024 * 1024;

#[derive(Deserialize)]
struct Route {
    #[serde(default)]
    dst: String,
    #[serde(default)]
    dev: String,
}

/// Evaluates the output of `ip -j route show table …`. An empty table produces no output.
pub fn routes_block_internet(output: &str) -> Result<bool, serde_json::Error> {
    if output.trim().is_empty() {
        return Ok(false);
    }
    let routes: Vec<Route> = serde_json::from_str(output)?;
    Ok(routes.iter().any(|route| route.dst == "default" && route.dev == "lo"))
}

fn read_routes(ipv6: bool) -> io::Result<String> {
    let family: &[&str] = if ipv6 { &["-6"] } else { &[] };
    let args = [family, &["-j", "route", "show", "table", ROUTING_TABLE]].concat();
    crate::process::run(&IP_COMMANDS, &args, MAX_OUTPUT)
}

/// Checks whether the blocking routes are currently set.
pub fn is_blocked() -> io::Result<bool> {
    for ipv6 in [false, true] {
        let output = read_routes(ipv6)?;
        if routes_block_internet(&output).map_err(|_| io::Error::from(io::ErrorKind::InvalidData))? {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Real output while the internet is blocked, taken from the real service (2026-11-08)
    const LOOPBACK_V4: &str =
        r#"[{"dst":"default","dev":"lo","flags":[]},{"type":"throw","dst":"255.255.255.255","flags":[]}]"#;
    const LOOPBACK_V6: &str = r#"[{"dst":"default","dev":"lo","metric":1024,"flags":[],"pref":"medium"}]"#;
    // Counterexamples: the tunnel route and the `throw` route alone do not mean a block
    const TUNNEL_ONLY: &str = r#"[{"dst":"default","dev":"vpn","flags":[]}]"#;
    const THROW_ROUTE: &str = r#"[{"type":"throw","dst":"255.255.255.255","flags":[]}]"#;

    #[test]
    fn leere_tabelle_ist_keine_sperre() {
        assert!(!routes_block_internet("").unwrap());
        assert!(!routes_block_internet("[ ]\n").unwrap());
    }

    #[test]
    fn loopback_standardroute_ist_eine_sperre() {
        assert!(routes_block_internet(LOOPBACK_V4).unwrap());
        assert!(routes_block_internet(LOOPBACK_V6).unwrap());
    }

    #[test]
    fn andere_routen_sind_keine_sperre() {
        assert!(!routes_block_internet(TUNNEL_ONLY).unwrap());
        assert!(!routes_block_internet(THROW_ROUTE).unwrap());
    }

    #[test]
    fn unlesbare_ausgabe_ist_ein_fehler() {
        assert!(routes_block_internet("kein json").is_err());
    }
}
