// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! Streaming servers of hide.me.
//!
//! hide.me does not publish these servers in any list that can be fetched without signing in. The
//! signed-in API `network/streaming/en` is ruled out because it would need credentials, which Celata
//! never handles. The table therefore comes from DNS research on 2026-11-08. At startup Celata checks
//! whether every name still exists and hides those that no longer do.

use std::collections::BTreeSet;
use std::net::ToSocketAddrs;

use crate::i18n::{Key, Language, Phrase};

/// An entry of the fixed table: country code (ISO 3166-1), abbreviation in the host name, English name.
struct Entry {
    code: &'static str,
    slug: &'static str,
    english: &'static str,
}

const fn entry(code: &'static str, slug: &'static str, english: &'static str) -> Entry {
    Entry { code, slug, english }
}

/// The 20 streaming servers from the research. The abbreviation is not always the country code:
/// the United Kingdom is `uk` in the host name, its country code is `GB`.
const ENTRIES: [Entry; 20] = [
    entry("AR", "ar", "Argentina"),
    entry("AT", "at", "Austria"),
    entry("AU", "au", "Australia"),
    entry("BR", "br", "Brazil"),
    entry("CA", "ca", "Canada"),
    entry("CH", "ch", "Switzerland"),
    entry("DE", "de", "Germany"),
    entry("ES", "es", "Spain"),
    entry("FR", "fr", "France"),
    entry("HR", "hr", "Croatia"),
    entry("IT", "it", "Italy"),
    entry("JP", "jp", "Japan"),
    entry("NL", "nl", "Netherlands"),
    entry("NO", "no", "Norway"),
    entry("PL", "pl", "Poland"),
    entry("PT", "pt", "Portugal"),
    entry("SE", "se", "Sweden"),
    entry("TR", "tr", "Turkey"),
    entry("GB", "uk", "United Kingdom"),
    entry("US", "us", "United States"),
];

/// Must resolve, otherwise no check is possible (no internet, block).
pub const POSITIVE_PROBE: &str = "nl.hideservers.net";
/// Must not resolve, otherwise the resolver answers unknown names as well.
/// Checked on 2026-11-12 at ns1.hideservers.net: no address.
pub const NEGATIVE_PROBE: &str = "streaming-celata-probe.hideservers.net";

/// A streaming server as Celata shows it. The country name is only created when showing, in the
/// language in force ([`StreamingServer::name`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamingServer {
    /// Country code, for example "GB".
    pub code: String,
    /// English country name of the table; fallback if a language does not know the code.
    pub english: String,
    /// Host name for `POST /configuration`, for example `streaming-uk.hideservers.net`.
    pub host: String,
}

impl StreamingServer {
    /// Country name in `language`.
    pub fn name(&self, language: Language) -> String {
        self.country().render(language)
    }

    /// Country name as a language-independent message.
    pub fn country(&self) -> Phrase {
        Phrase::country(&self.code, &self.english)
    }
}

fn host_of(entry: &Entry) -> String {
    format!("streaming-{}.hideservers.net", entry.slug)
}

/// All streaming servers of the table, in the order of the table. Sorting happens when showing, by
/// the name in the language in force (`look::streaming_rows`).
pub fn all() -> Vec<StreamingServer> {
    ENTRIES
        .iter()
        .map(|entry| StreamingServer {
            code: entry.code.to_owned(),
            english: entry.english.to_owned(),
            host: host_of(entry),
        })
        .collect()
}

/// Result of the check at startup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Check {
    /// The check is still running or has not started: all entries visible.
    Pending,
    /// A control name behaved wrongly, or more than half are missing: all entries visible.
    Unreliable,
    /// Both controls passed; the host names without an address are hidden.
    Checked(BTreeSet<String>),
}

/// Checks the names. `resolves` tells whether a name has an address.
///
/// The names are queried one after the other. On 2026-11-08 a router answered simultaneous queries
/// for five existing servers with "unknown". A name without an address is queried a second time at
/// the end, so that a single dropout does not hide a server. The check runs in the background and
/// may therefore take a while (a good 20 seconds with slow answers to IPv6 queries).
pub fn check(resolves: impl Fn(&str) -> bool) -> Check {
    if !resolves(POSITIVE_PROBE) || resolves(NEGATIVE_PROBE) {
        return Check::Unreliable;
    }
    let first: Vec<String> = ENTRIES.iter().map(host_of).filter(|host| !resolves(host)).collect();
    let missing: BTreeSet<String> = first.into_iter().filter(|host| !resolves(host)).collect();
    if missing.len() * 2 > ENTRIES.len() {
        return Check::Unreliable;
    }
    Check::Checked(missing)
}

/// The streaming servers that are shown according to the result of the check, in the order of [`all`].
pub fn visible(check: &Check) -> Vec<StreamingServer> {
    let mut servers = all();
    if let Check::Checked(missing) = check {
        servers.retain(|server| !missing.contains(&server.host));
    }
    servers
}

/// Asks the name resolution of the system. Kind and text of an error are deliberately neither
/// evaluated nor logged: whether an error means "name unknown" or "no network" is settled by the
/// control names in [`check`].
pub fn resolves(host: &str) -> bool {
    (host, 443)
        .to_socket_addrs()
        .is_ok_and(|mut addresses| addresses.next().is_some())
}

/// Describes a connected host for the status display, for example "Streaming Japan".
/// `None` for hosts that are not a streaming server of the table.
pub fn describe_host(host: &str) -> Option<Phrase> {
    all()
        .into_iter()
        .find(|server| server.host == host)
        .map(|server| Phrase::fill(Key::StreamingName, [server.country()]))
}

/// Text after "Server:" and in messages: streaming server with country, for example
/// "Streaming United Kingdom (streaming-uk.hideservers.net)", otherwise the host name.
pub fn server_label(host: &str) -> Phrase {
    match describe_host(host) {
        Some(description) => Phrase::fill(Key::StreamingLabel, [description, Phrase::raw(host)]),
        None => Phrase::raw(host),
    }
}

/// Short label for narrow places such as the sidebar: "Streaming United Kingdom" without the host
/// name, otherwise the host name.
pub fn short_label(host: &str) -> Phrase {
    describe_host(host).unwrap_or_else(|| Phrase::raw(host))
}

/// Searches case-insensitively in the country name of all languages and in the English name of the
/// table. An empty search term returns all entries passed in.
pub fn search(servers: &[StreamingServer], query: &str) -> Vec<StreamingServer> {
    let query = query.trim().to_lowercase();
    servers
        .iter()
        .filter(|server| {
            query.is_empty()
                || server.english.to_lowercase().contains(&query)
                || Language::ALL
                    .into_iter()
                    .any(|language| server.name(language).to_lowercase().contains(&query))
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::servers::is_valid_host;

    fn hosts(servers: &[StreamingServer]) -> Vec<&str> {
        servers.iter().map(|server| server.host.as_str()).collect()
    }

    // --- Table ---

    #[test]
    fn genau_zwanzig_eintraege() {
        assert_eq!(all().len(), 20);
    }

    #[test]
    fn kein_code_und_kein_kuerzel_doppelt() {
        let codes: BTreeSet<_> = ENTRIES.iter().map(|entry| entry.code).collect();
        let slugs: BTreeSet<_> = ENTRIES.iter().map(|entry| entry.slug).collect();
        assert_eq!(codes.len(), 20);
        assert_eq!(slugs.len(), 20);
    }

    #[test]
    fn jeder_hostname_ist_gueltig() {
        for server in all() {
            assert!(is_valid_host(&server.host), "{}", server.host);
            assert!(server.host.starts_with("streaming-"), "{}", server.host);
        }
    }

    #[test]
    fn jeder_code_ist_bekannt_und_hat_namen() {
        for entry in &ENTRIES {
            assert!(crate::names::is_known(entry.code), "{}", entry.code);
        }
        let uk = all().into_iter().find(|server| server.code == "GB").unwrap();
        assert_eq!(uk.host, "streaming-uk.hideservers.net");
        assert_eq!(uk.name(Language::De), "Vereinigtes Königreich");
        assert_eq!(uk.name(Language::En), "United Kingdom");
    }

    // --- Check at startup ---

    /// Imitated name resolution: control names as given; of the streaming names the first `missing` of
    /// the table are absent.
    fn resolver(positive: bool, negative: bool, missing: usize) -> impl Fn(&str) -> bool {
        let gone: Vec<String> = ENTRIES.iter().take(missing).map(host_of).collect();
        move |host: &str| match host {
            POSITIVE_PROBE => positive,
            NEGATIVE_PROBE => negative,
            other => !gone.iter().any(|known| known == other),
        }
    }

    #[test]
    fn alles_da() {
        let result = check(resolver(true, false, 0));
        assert_eq!(result, Check::Checked(BTreeSet::new()));
        assert_eq!(visible(&result).len(), 20);
    }

    #[test]
    fn ein_server_abgeschaltet() {
        let result = check(resolver(true, false, 1));
        let expected: BTreeSet<String> = [host_of(&ENTRIES[0])].into();
        assert_eq!(result, Check::Checked(expected));
        let shown = visible(&result);
        assert_eq!(shown.len(), 19);
        assert!(!hosts(&shown).contains(&"streaming-ar.hideservers.net"));
    }

    #[test]
    fn kein_internet_oder_sperre() {
        // Without a network nothing resolves, none of the streaming names either.
        let result = check(|_: &str| false);
        assert_eq!(result, Check::Unreliable);
        assert_eq!(visible(&result).len(), 20);
    }

    #[test]
    fn resolver_beantwortet_alles() {
        let result = check(resolver(true, true, 0));
        assert_eq!(result, Check::Unreliable);
        assert_eq!(visible(&result).len(), 20);
    }

    #[test]
    fn mehr_als_die_haelfte_fehlt() {
        let result = check(resolver(true, false, 11));
        assert_eq!(result, Check::Unreliable);
        assert_eq!(visible(&result).len(), 20);
    }

    #[test]
    fn genau_die_haelfte_fehlt() {
        let result = check(resolver(true, false, 10));
        assert!(matches!(&result, Check::Checked(missing) if missing.len() == 10));
        assert_eq!(visible(&result).len(), 10);
    }

    #[test]
    fn einzelner_aussetzer_blendet_nichts_aus() {
        // The first attempt for streaming-ar fails, the second succeeds.
        let tries = std::cell::Cell::new(0);
        let result = check(|host: &str| match host {
            POSITIVE_PROBE => true,
            NEGATIVE_PROBE => false,
            "streaming-ar.hideservers.net" => {
                tries.set(tries.get() + 1);
                tries.get() > 1
            }
            _ => true,
        });
        assert_eq!(result, Check::Checked(BTreeSet::new()));
        assert_eq!(tries.get(), 2);
    }

    #[test]
    fn ausstehend_zeigt_alle_sortiert() {
        assert_eq!(visible(&Check::Pending), all());
    }

    #[test]
    fn kontrollnamen_werden_zuerst_geprueft() {
        // If the positive control fails, the streaming names are not queried at all.
        let asked = std::cell::RefCell::new(Vec::new());
        check(|host: &str| {
            asked.borrow_mut().push(host.to_owned());
            false
        });
        assert_eq!(*asked.borrow(), vec![POSITIVE_PROBE.to_owned()]);
    }

    // --- Status display ---

    #[test]
    fn beschreibung_fuer_den_status() {
        let describe = |host, language| describe_host(host).map(|phrase| phrase.render(language));
        assert_eq!(
            describe("streaming-uk.hideservers.net", Language::De).as_deref(),
            Some("Streaming Vereinigtes Königreich")
        );
        assert_eq!(
            describe("streaming-uk.hideservers.net", Language::En).as_deref(),
            Some("Streaming United Kingdom")
        );
        assert_eq!(
            describe("streaming-jp.hideservers.net", Language::De).as_deref(),
            Some("Streaming Japan")
        );
        assert_eq!(describe_host("nl.hideservers.net"), None);
        assert_eq!(describe_host("streaming.hideservers.net"), None);
    }

    #[test]
    fn beschriftung_hinter_server() {
        assert_eq!(
            server_label("streaming-uk.hideservers.net").render(Language::De),
            "Streaming Vereinigtes Königreich (streaming-uk.hideservers.net)"
        );
        assert_eq!(
            server_label("streaming-uk.hideservers.net").render(Language::En),
            "Streaming United Kingdom (streaming-uk.hideservers.net)"
        );
        assert_eq!(
            server_label("nl.hideservers.net").render(Language::De),
            "nl.hideservers.net"
        );
        assert_eq!(
            short_label("streaming-uk.hideservers.net").render(Language::En),
            "Streaming United Kingdom"
        );
        assert_eq!(
            short_label("nl.hideservers.net").render(Language::En),
            "nl.hideservers.net"
        );
    }

    // --- Search ---

    fn found(query: &str) -> Vec<String> {
        search(&all(), query)
            .into_iter()
            .map(|server| server.name(Language::De))
            .collect()
    }

    #[test]
    fn suche_deutsch_englisch_und_gross_klein() {
        assert_eq!(found("japan"), vec!["Japan"]);
        assert_eq!(found("JAPAN"), vec!["Japan"]);
        assert_eq!(found("deutsch"), vec!["Deutschland"]);
        assert_eq!(found("germany"), vec!["Deutschland"]);
        assert_eq!(found("  königreich "), vec!["Vereinigtes Königreich"]);
        assert_eq!(found("kingdom"), vec!["Vereinigtes Königreich"]);
    }

    #[test]
    fn leere_suche_und_keine_treffer() {
        assert_eq!(found("  ").len(), 20);
        assert!(found("Atlantis").is_empty());
    }

    #[test]
    fn suche_nur_in_den_sichtbaren() {
        let shown = visible(&check(resolver(true, false, 1)));
        assert!(search(&shown, "argent").is_empty());
    }
}
