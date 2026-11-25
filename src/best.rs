// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! "Best location": the nearest single place of the loaded server list.
//!
//! Pure logic without any access to the system. The location of the user comes from `zone.rs`, the server
//! list from `servers.rs`. Only the distance counts; there are no measurements, no randomness, no clock and
//! no state besides the inputs, so that the same inputs always give the same order. Nothing of it is
//! stored; only the host chosen in the end goes to the service, like for every location.

use std::collections::BTreeSet;

use crate::i18n::{self, Key, Language};
use crate::servers::{Country, Position, Server};

/// Mark for "Best location" in the selection, in its row and in the favourites. It can never be a host name,
/// because every valid host ends in `.hideservers.net`.
pub const BEST: &str = "best";
/// Candidates checked one after another during a switch.
pub const MAX_TRIES: usize = 3;
/// Mean radius of the earth.
const EARTH_RADIUS_KM: f64 = 6371.0;

/// Where the user is, as far as this computer tells.
#[derive(Debug, Clone, PartialEq)]
pub enum Origin {
    /// Place of the configured time zone, in degrees.
    Point { lat: f64, lon: f64 },
    /// Country code of the system language, for example "DE", if the time zone has no place.
    Country(String),
}

/// Why no best location can be determined.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unavailable {
    /// The list is still loading and has no countries yet.
    Loading,
    /// There is no list (loading failed).
    NoList,
    /// Neither the time zone nor the system language gives a location.
    NoOrigin,
    /// No candidate is left (for example with "Free only", or a country without candidates).
    NoCandidates,
}

/// Great-circle distance in kilometres between two points in degrees (haversine formula).
pub fn distance_km(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (lat_a, lat_b) = (a.0.to_radians(), b.0.to_radians());
    let half_lat = (lat_b - lat_a) / 2.0;
    let half_lon = (b.1 - a.1).to_radians() / 2.0;
    let h = half_lat.sin().powi(2) + lat_a.cos() * lat_b.cos() * half_lon.sin().powi(2);
    2.0 * EARTH_RADIUS_KM * h.clamp(0.0, 1.0).sqrt().asin()
}

/// Simple mean of latitude and longitude, or `None` without points.
fn mean(points: impl Iterator<Item = (f64, f64)>) -> Option<(f64, f64)> {
    let (count, lat, lon) = points.fold((0_u32, 0.0, 0.0), |(count, lat, lon), point| {
        (count + 1, lat + point.0, lon + point.1)
    });
    (count > 0).then(|| (lat / f64::from(count), lon / f64::from(count)))
}

/// A server that may become the best location, with the place used for the calculation.
struct Candidate<'a> {
    code: &'a str,
    host: &'a str,
    point: (f64, f64),
}

/// May `server` become the best location at all?
fn eligible(server: &Server, free_only: bool, excluded: &BTreeSet<String>) -> bool {
    !server.automatic
        && !server.derived
        && !server.no_best
        && server.free == free_only
        && !excluded.contains(&server.host)
}

/// All candidates in the order of the list. A server without coordinates gets the mean of all servers of its
/// country that have some (free and paid, also flagged or excluded ones); without any it is no candidate.
fn candidates<'a>(countries: &'a [Country], free_only: bool, excluded: &BTreeSet<String>) -> Vec<Candidate<'a>> {
    let mut found = Vec::new();
    for country in countries {
        let positions = || {
            country
                .servers
                .iter()
                .filter_map(|server| server.position.map(Position::degrees))
        };
        let replacement = mean(positions());
        for server in country
            .servers
            .iter()
            .filter(|server| eligible(server, free_only, excluded))
        {
            if let Some(point) = server.position.map(Position::degrees).or(replacement) {
                found.push(Candidate {
                    code: &country.code,
                    host: &server.host,
                    point,
                });
            }
        }
    }
    found
}

/// All candidates as hosts, best first (contract `ermittlung.md`, sections 3 and 4).
///
/// `Origin::Point`: sorted by distance to the point. `Origin::Country(c)`: the point is the mean of the
/// candidates of `c`; first the candidates of `c`, then all others, each by distance to that point. Equal
/// distances keep the order of the list.
pub fn rank(
    countries: &[Country],
    origin: Option<&Origin>,
    loading: bool,
    free_only: bool,
    excluded: &BTreeSet<String>,
) -> Result<Vec<String>, Unavailable> {
    if countries.is_empty() {
        return Err(if loading {
            Unavailable::Loading
        } else {
            Unavailable::NoList
        });
    }
    let origin = origin.ok_or(Unavailable::NoOrigin)?;
    let candidates = candidates(countries, free_only, excluded);
    let (point, home) = match origin {
        Origin::Point { lat, lon } => ((*lat, *lon), None),
        Origin::Country(code) => {
            let inside = candidates.iter().filter(|candidate| candidate.code == code);
            let point = mean(inside.map(|candidate| candidate.point)).ok_or(Unavailable::NoCandidates)?;
            (point, Some(code.as_str()))
        }
    };
    let mut ranked: Vec<(bool, f64, &str)> = candidates
        .iter()
        .map(|candidate| {
            let abroad = home.is_some_and(|home| candidate.code != home);
            (abroad, distance_km(point, candidate.point), candidate.host)
        })
        .collect();
    // `sort_by` is stable: equal distances keep the order of the list.
    ranked.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.total_cmp(&b.1)));
    if ranked.is_empty() {
        return Err(Unavailable::NoCandidates);
    }
    Ok(ranked.into_iter().map(|(_, _, host)| host.to_owned()).collect())
}

/// Does `host` stand for "Best location"? An empty selection counts as well.
pub fn is_best(host: &str) -> bool {
    host.is_empty() || host == BEST
}

/// Does the search term match the name "Best location" in any language? An empty term matches.
pub fn matches(query: &str) -> bool {
    let query = query.trim().to_lowercase();
    query.is_empty()
        || Language::ALL
            .into_iter()
            .any(|language| i18n::text(language, Key::BestName).to_lowercase().contains(&query))
}

/// What connecting through "Best location" does; `main.rs` only carries it out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    /// Show this notice; no call to the service.
    Inform(Key),
    /// The connected host is already the best one.
    Already(String),
    /// Connect or switch to `host`; `rest` holds at most [`MAX_TRIES`] − 1 further candidates for the check of
    /// names during a switch.
    Go { host: String, rest: Vec<String> },
}

/// Decides from the order and the connected host.
pub fn choose(ranked: Result<Vec<String>, Unavailable>, connected: Option<&str>) -> Choice {
    let hosts = match ranked {
        Ok(hosts) => hosts,
        Err(Unavailable::Loading) => return Choice::Inform(Key::NoticeBestLoading),
        Err(Unavailable::NoList) => return Choice::Inform(Key::NoticeBestNoList),
        Err(Unavailable::NoOrigin | Unavailable::NoCandidates) => return Choice::Inform(Key::NoticeBestUnknown),
    };
    let mut hosts = hosts.into_iter().take(MAX_TRIES);
    let Some(host) = hosts.next() else {
        return Choice::Inform(Key::NoticeBestUnknown);
    };
    match connected {
        Some(connected) if connected.eq_ignore_ascii_case(&host) => Choice::Already(connected.to_owned()),
        _ => Choice::Go {
            host,
            rest: hosts.collect(),
        },
    }
}

/// What "Best location" remembers during the session; never stored.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Memory {
    /// Host chosen last through "Best location", only for "Chosen: …".
    host: Option<String>,
    /// Hosts chosen through "Best location" that failed, and candidates without an address.
    failed: BTreeSet<String>,
}

impl Memory {
    /// A connection or switch to `host` starts; `via_best` if "Best location" chose it, otherwise the user
    /// chose the location and "Chosen: …" disappears.
    pub fn started(&mut self, via_best: bool, host: &str) {
        self.host = via_best.then(|| host.to_owned());
    }

    /// Leaves `host` out of "Best location" until the end of the session.
    pub fn failed(&mut self, host: &str) {
        self.failed.insert(host.to_owned());
    }

    pub fn host(&self) -> Option<&str> {
        self.host.as_deref()
    }

    pub fn excluded(&self) -> &BTreeSet<String> {
        &self.failed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::servers::parse_list;
    use crate::zone;
    use std::time::{Duration, Instant};

    const LIST: &[u8] = include_bytes!("../tests/fixtures/servers-2026-11-19.json");
    const ZONE_TAB: &str = include_str!("../tests/fixtures/tz/zone.tab");
    const ZONE1970_TAB: &str = include_str!("../tests/fixtures/tz/zone1970.tab");
    const LINKS: &str = include_str!("../tests/fixtures/tz/links.zi");

    static NONE: BTreeSet<String> = BTreeSet::new();

    fn real() -> Vec<Country> {
        parse_list(LIST).unwrap()
    }

    fn zone(name: &str) -> Origin {
        let (lat, lon) = zone::lookup(&[ZONE_TAB, ZONE1970_TAB], LINKS, name).unwrap();
        Origin::Point { lat, lon }
    }

    fn first(origin: &Origin, free_only: bool) -> String {
        rank(&real(), Some(origin), false, free_only, &NONE).unwrap().remove(0)
    }

    fn all_servers(countries: &[Country]) -> impl Iterator<Item = &Server> {
        countries.iter().flat_map(|country| &country.servers)
    }

    #[test]
    fn entfernungen() {
        let paris = (48.8566, 2.3522);
        let berlin = (52.52, 13.405);
        let distance = distance_km(berlin, paris);
        assert!((distance - 878.0).abs() < 5.0, "{distance}");
        assert!(distance_km(berlin, berlin).abs() < 1e-9);
        let antipodes = distance_km((10.0, 20.0), (-10.0, -160.0));
        assert!((antipodes - 20015.0).abs() < 1.0, "{antipodes}");
        assert!((distance_km(paris, berlin) - distance).abs() < 1e-9);
    }

    #[test]
    fn berlin_waehlt_berlin_und_kostenlos_frankfurt() {
        assert_eq!(first(&zone("Europe/Berlin"), false), "berlin.hideservers.net");
        assert_eq!(first(&zone("Europe/Berlin"), true), "free-de.hideservers.net");
    }

    #[test]
    fn kopenhagen_und_new_york() {
        assert_eq!(first(&zone("Europe/Copenhagen"), false), "dk.hideservers.net");
        assert_eq!(first(&zone("America/New_York"), false), "newyorkcity.hideservers.net");
        // The free server of the USA has no coordinates of its own: the mean of the US places applies.
        assert_eq!(first(&zone("America/New_York"), true), "free-us.hideservers.net");
    }

    #[test]
    fn ausgeschlossen_sind_no_best_automatisch_gebildet_und_gescheiterte() {
        let countries = real();
        let flagged: BTreeSet<&str> = all_servers(&countries)
            .filter(|server| server.no_best || server.automatic || server.derived)
            .map(|server| server.host.as_str())
            .collect();
        assert!(flagged.contains("free-fi.hideservers.net"));
        assert!(flagged.contains("fi.hideservers.net"));
        assert!(flagged.contains("de.hideservers.net"));
        for free_only in [false, true] {
            for name in ["Europe/Helsinki", "Europe/London", "Europe/Berlin", "Asia/Tokyo"] {
                let ranked = rank(&countries, Some(&zone(name)), false, free_only, &NONE).unwrap();
                assert!(ranked.iter().all(|host| !flagged.contains(host.as_str())), "{name}");
            }
        }
        let excluded: BTreeSet<String> = ["berlin.hideservers.net".to_owned()].into();
        let ranked = rank(&countries, Some(&zone("Europe/Berlin")), false, false, &excluded).unwrap();
        assert!(!ranked.contains(&"berlin.hideservers.net".to_owned()));
        // Without Berlin, Prague (about 270 km) is nearer than Frankfurt (about 420 km).
        assert_eq!(ranked[0], "cz.hideservers.net");
    }

    #[test]
    fn nur_kostenlose_oder_nur_bezahlte() {
        let countries = real();
        let free: BTreeSet<&str> = all_servers(&countries)
            .filter(|server| server.free)
            .map(|server| server.host.as_str())
            .collect();
        let origin = zone("Europe/Berlin");
        let paid = rank(&countries, Some(&origin), false, false, &NONE).unwrap();
        assert!(paid.iter().all(|host| !free.contains(host.as_str())));
        assert_eq!(paid.len(), 87);
        let only_free = rank(&countries, Some(&origin), false, true, &NONE).unwrap();
        assert!(only_free.iter().all(|host| free.contains(host.as_str())));
        assert_eq!(
            only_free,
            [
                "free-de.hideservers.net",
                "free-nl.hideservers.net",
                "free-ch.hideservers.net",
                "free-fr.hideservers.net",
                "free-us.hideservers.net"
            ]
        );
    }

    #[test]
    fn gleichstand_in_der_reihenfolge_der_liste() {
        // Frankfurt #1 and #2 have the same coordinates.
        let frankfurt = Origin::Point {
            lat: 50.1109,
            lon: 8.6821,
        };
        let ranked = rank(&real(), Some(&frankfurt), false, false, &NONE).unwrap();
        assert_eq!(
            ranked[..2],
            ["frankfurt1.hideservers.net", "frankfurt2.hideservers.net"]
        );
    }

    #[test]
    fn zehnmal_dieselbe_rangfolge() {
        let countries = real();
        let origin = zone("Europe/Berlin");
        let once = rank(&countries, Some(&origin), false, false, &NONE).unwrap();
        for _ in 0..10 {
            assert_eq!(rank(&countries, Some(&origin), false, false, &NONE).unwrap(), once);
        }
    }

    #[test]
    fn rueckfall_ueber_das_land() {
        let countries = real();
        let germany = Origin::Country("DE".to_owned());
        let ranked = rank(&countries, Some(&germany), false, false, &NONE).unwrap();
        assert_eq!(ranked[0], "frankfurt1.hideservers.net");
        let german: BTreeSet<&str> = countries
            .iter()
            .filter(|country| country.code == "DE")
            .flat_map(|country| &country.servers)
            .map(|server| server.host.as_str())
            .collect();
        // First all German candidates (Frankfurt #1, #2, Berlin), then the others.
        assert!(
            ranked[..3].iter().all(|host| german.contains(host.as_str())),
            "{ranked:?}"
        );
        assert!(
            ranked[3..].iter().all(|host| !german.contains(host.as_str())),
            "{ranked:?}"
        );
        for (code, expected) in [
            ("US", "kansascity.hideservers.net"),
            ("FR", "bordeaux.hideservers.net"),
            ("GB", "manchester.hideservers.net"),
            ("CH", "zurich.hideservers.net"),
        ] {
            let origin = Origin::Country(code.to_owned());
            assert_eq!(
                rank(&countries, Some(&origin), false, false, &NONE).unwrap()[0],
                expected,
                "{code}"
            );
        }
        let austria = Origin::Country("AT".to_owned());
        assert_eq!(
            rank(&countries, Some(&austria), false, true, &NONE),
            Err(Unavailable::NoCandidates)
        );
        let unknown = Origin::Country("XX".to_owned());
        assert_eq!(
            rank(&countries, Some(&unknown), false, false, &NONE),
            Err(Unavailable::NoCandidates)
        );
    }

    #[test]
    fn ohne_liste_oder_ort() {
        let origin = zone("Europe/Berlin");
        assert_eq!(rank(&[], Some(&origin), true, false, &NONE), Err(Unavailable::Loading));
        assert_eq!(rank(&[], Some(&origin), false, false, &NONE), Err(Unavailable::NoList));
        assert_eq!(rank(&[], None, false, false, &NONE), Err(Unavailable::NoList));
        assert_eq!(rank(&real(), None, false, false, &NONE), Err(Unavailable::NoOrigin));
        // Loading again while a list is shown: the list counts.
        assert_eq!(
            rank(&real(), Some(&origin), true, false, &NONE).unwrap()[0],
            "berlin.hideservers.net"
        );
        // Every candidate excluded.
        let all: BTreeSet<String> = rank(&real(), Some(&origin), false, true, &NONE)
            .unwrap()
            .into_iter()
            .collect();
        assert_eq!(
            rank(&real(), Some(&origin), false, true, &all),
            Err(Unavailable::NoCandidates)
        );
    }

    /// For every time zone of the database the first host is the candidate with the smallest distance,
    /// checked against a plain comparison over all servers.
    #[test]
    fn jede_zeitzone_ergibt_den_naechsten_kandidaten() {
        let countries = real();
        let mut names = BTreeSet::new();
        for table in [ZONE_TAB, ZONE1970_TAB] {
            for line in table.lines().filter(|line| !line.starts_with('#')) {
                if let Some(name) = line.split('\t').nth(2) {
                    names.insert(name.to_owned());
                }
            }
        }
        assert!(names.len() > 400, "{}", names.len());
        let point_of = |country: &Country, server: &Server| {
            server.position.map(Position::degrees).or_else(|| {
                mean(
                    country
                        .servers
                        .iter()
                        .filter_map(|other| other.position.map(Position::degrees)),
                )
            })
        };
        for free_only in [false, true] {
            for name in &names {
                let origin = zone(name);
                let Origin::Point { lat, lon } = origin else {
                    unreachable!()
                };
                let mut best: Option<(f64, &str)> = None;
                for country in &countries {
                    for server in &country.servers {
                        if server.automatic || server.derived || server.no_best || server.free != free_only {
                            continue;
                        }
                        let Some(point) = point_of(country, server) else {
                            continue;
                        };
                        let distance = distance_km((lat, lon), point);
                        if best.is_none_or(|(known, _)| distance < known) {
                            best = Some((distance, &server.host));
                        }
                    }
                }
                let ranked = rank(&countries, Some(&origin), false, free_only, &NONE).unwrap();
                assert_eq!(ranked[0], best.unwrap().1, "{name}, nur kostenlose: {free_only}");
            }
        }
    }

    /// Reading the tables and ranking take far less than 0.1 seconds.
    #[test]
    fn ermittlung_ist_schnell() {
        let countries = real();
        let start = Instant::now();
        let (lat, lon) = zone::lookup(&[ZONE_TAB, ZONE1970_TAB], LINKS, "US/Central").unwrap();
        let ranked = rank(&countries, Some(&Origin::Point { lat, lon }), false, false, &NONE).unwrap();
        assert!(!ranked.is_empty());
        assert!(start.elapsed() < Duration::from_millis(100), "{:?}", start.elapsed());
    }

    #[test]
    fn marke_und_suche() {
        assert!(is_best("best"));
        assert!(is_best(""));
        assert!(!is_best("berlin.hideservers.net"));
        assert!(!is_best("BEST"));
        for query in [
            "best",
            "bester",
            "BEST",
            " Standort ",
            "",
            "  ",
            "location",
            "Bester Standort",
        ] {
            assert!(matches(query), "{query}");
        }
        for query in ["berlin", "beste standorte", "automatic"] {
            assert!(!matches(query), "{query}");
        }
    }

    fn hosts(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| format!("{name}.hideservers.net")).collect()
    }

    #[test]
    fn entscheidung_ohne_ermittlung() {
        assert_eq!(
            choose(Err(Unavailable::Loading), None),
            Choice::Inform(Key::NoticeBestLoading)
        );
        assert_eq!(
            choose(Err(Unavailable::NoList), None),
            Choice::Inform(Key::NoticeBestNoList)
        );
        assert_eq!(
            choose(Err(Unavailable::NoOrigin), None),
            Choice::Inform(Key::NoticeBestUnknown)
        );
        assert_eq!(
            choose(Err(Unavailable::NoCandidates), Some("berlin.hideservers.net")),
            Choice::Inform(Key::NoticeBestUnknown)
        );
        assert_eq!(choose(Ok(Vec::new()), None), Choice::Inform(Key::NoticeBestUnknown));
    }

    #[test]
    fn entscheidung_beim_verbinden() {
        let ranked = hosts(&["berlin", "frankfurt1", "frankfurt2", "prague", "vienna"]);
        assert_eq!(
            choose(Ok(ranked.clone()), None),
            Choice::Go {
                host: "berlin.hideservers.net".to_owned(),
                rest: hosts(&["frankfurt1", "frankfurt2"])
            }
        );
        assert_eq!(
            choose(Ok(ranked.clone()), Some("BERLIN.hideservers.net")),
            Choice::Already("BERLIN.hideservers.net".to_owned())
        );
        assert_eq!(
            choose(Ok(ranked), Some("nl.hideservers.net")),
            Choice::Go {
                host: "berlin.hideservers.net".to_owned(),
                rest: hosts(&["frankfurt1", "frankfurt2"])
            }
        );
        let single = hosts(&["berlin"]);
        assert_eq!(
            choose(Ok(single.clone()), Some("berlin.hideservers.net")),
            Choice::Already("berlin.hideservers.net".to_owned())
        );
        assert_eq!(
            choose(Ok(single), Some("nl.hideservers.net")),
            Choice::Go {
                host: "berlin.hideservers.net".to_owned(),
                rest: Vec::new()
            }
        );
        // Never more than two further candidates.
        for count in 1..6 {
            let names: Vec<String> = (0..count).map(|index| format!("h{index}.hideservers.net")).collect();
            let Choice::Go { rest, .. } = choose(Ok(names), None) else {
                panic!()
            };
            assert_eq!(rest.len(), (count - 1).min(MAX_TRIES - 1));
        }
    }

    #[test]
    fn gedaechtnis_der_sitzung() {
        let mut memory = Memory::default();
        assert_eq!(memory.host(), None);
        assert!(memory.excluded().is_empty());
        memory.started(true, "berlin.hideservers.net");
        assert_eq!(memory.host(), Some("berlin.hideservers.net"));
        // A failure does not change the host shown, only the exclusion.
        memory.failed("berlin.hideservers.net");
        assert_eq!(memory.host(), Some("berlin.hideservers.net"));
        assert!(memory.excluded().contains("berlin.hideservers.net"));
        // An own choice removes "Chosen: …", the exclusion stays.
        memory.started(false, "nl.hideservers.net");
        assert_eq!(memory.host(), None);
        assert!(memory.excluded().contains("berlin.hideservers.net"));
    }
}
