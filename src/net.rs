// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! The one way Celata fetches public data from the internet: HTTPS only, bounded in time and size.
//!
//! Used for the public server list and the public category list. Nothing is sent besides the request itself.

use std::time::Duration;

/// Longest time a fetch may take as a whole.
pub const TIMEOUT: Duration = Duration::from_secs(15);

/// Why a fetch failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchError {
    /// The address is not `https://`, or the server redirected to an unencrypted address.
    NotHttps,
    /// The server is not reachable or answered with an error.
    Network,
    /// The answer is larger than allowed.
    TooLarge,
}

/// Fetches `url` and returns the body, at most `limit` bytes.
pub fn get(url: &str, limit: u64) -> Result<Vec<u8>, FetchError> {
    if !url.starts_with("https://") {
        return Err(FetchError::NotHttps);
    }
    let map = |error: ureq::Error| match error {
        ureq::Error::RequireHttpsOnly(_) => FetchError::NotHttps,
        ureq::Error::BodyExceedsLimit(_) => FetchError::TooLarge,
        _ => FetchError::Network,
    };
    // `https_only` also refuses a redirect to an unencrypted address.
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .https_only(true)
        .timeout_global(Some(TIMEOUT))
        .build()
        .into();
    let mut response = agent.get(url).call().map_err(map)?;
    response
        .body_mut()
        .with_config()
        .limit(limit)
        .read_to_vec()
        .map_err(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unencrypted_addresses_are_refused_before_any_connection() {
        // Port 9 (discard) on the own machine: even if a connection were tried, nothing sensitive leaves.
        assert_eq!(get("http://127.0.0.1:9/list", 100), Err(FetchError::NotHttps));
        assert_eq!(get("ftp://127.0.0.1:9/list", 100), Err(FetchError::NotHttps));
        assert_eq!(get("127.0.0.1:9/list", 100), Err(FetchError::NotHttps));
    }

    #[test]
    fn unreachable_server_is_a_network_error() {
        // Nothing listens on port 9 of the own machine.
        assert_eq!(get("https://127.0.0.1:9/list", 100), Err(FetchError::Network));
    }
}
