// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! Server list of hide.me.
//!
//! The list comes directly from the public API and never from the service: `GET /serverList` would
//! overwrite its configuration (eventure/hide.client.linux, issue #61).

use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::continent::Continent;
use crate::files::{self, unix_seconds};
use crate::i18n::{self, Key, Language, Phrase};
use crate::names;
use crate::net::{self, FetchError};
use crate::text;

const HOST_SUFFIX: &str = ".hideservers.net";
const V4_SUFFIX: &str = "-v4.hideservers.net";

/// Public server list. It needs no sign-in and is the same one the client itself uses.
pub const LIST_URL: &str = "https://api.hide.me/v1/network/free/en";
/// The cache is valid for this long (`max-age=14400` of the API).
pub const CACHE_MAX_AGE: Duration = Duration::from_hours(4);
/// Largest permitted response of the API. The real list has about 40 KiB.
pub const MAX_LIST_SIZE: u64 = 1024 * 1024;
/// Longest name of a country, place or server that goes from the list into the display.
const MAX_NAME: usize = 64;

/// Checks that a host name has the form `^[a-z0-9-]+\.hideservers\.net$`.
pub fn is_valid_host(host: &str) -> bool {
    host.strip_suffix(HOST_SUFFIX).is_some_and(|name| {
        !name.is_empty()
            && name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    })
}

/// Derives the host name for the service like `hide.me list`: remove `-v4.hideservers.net` and
/// append `.hideservers.net`. Returns `None` if the result does not have the permitted form.
pub fn service_host(hostname: &str) -> Option<String> {
    let host = match hostname.strip_suffix(V4_SUFFIX) {
        Some(short) => format!("{short}{HOST_SUFFIX}"),
        None => hostname.to_owned(),
    };
    is_valid_host(&host).then_some(host)
}

/// What a server is called in the list. Translated only when it is shown ([`Server::name`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerLabel {
    /// Main entry of a country with several places: "Automatic".
    Automatic,
    /// Place with the name from the API, for example "Milan #2".
    Place(String),
    /// Only server of a country without a city, free of charge.
    Free,
    /// Only server of a country without a city.
    Plain,
}

/// A server that Celata can connect to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Server {
    /// Display name in the list, for example "Milan", "Helsinki #2" or "Automatic".
    pub label: ServerLabel,
    /// Display name from the API (`displayName`), for example "Milan" or "Germany - Free".
    pub original: String,
    /// City from `geo.cityName`, as the API names it (English).
    pub city: Option<String>,
    /// Host name for `POST /configuration`, checked with [`is_valid_host`].
    pub host: String,
    /// Free server (flag `free`). Other flags are not taken over.
    pub free: bool,
    /// Main entry of a country with several places: hide.me chooses the place itself.
    pub automatic: bool,
    /// Country name built by Celata because the list has no main entry for a country with several places
    /// ([`country_host`]); `automatic` is then true as well.
    pub derived: bool,
    /// Coordinates from `geo.lat` and `geo.lon`: both present, finite, latitude in −90…90 and longitude in
    /// −180…180; otherwise `None`.
    pub position: Option<Position>,
    /// Flag `no-best`: never chosen as "Best location".
    pub no_best: bool,
    /// Continent from `geo.continent`; `None` if missing, not a text or an unknown code, and for built names.
    pub continent: Option<Continent>,
}

/// Position on the earth in millionths of a degree (rounded to the nearest whole number). Whole numbers keep
/// `Server` comparable with `Eq`; a millionth of a degree is about 0.1 m.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Position {
    pub lat: i32,
    pub lon: i32,
}

/// Millionths of a degree per degree.
const MICRO: f64 = 1_000_000.0;

impl Position {
    /// Position from degrees, or `None` if a value is not finite or out of range.
    pub fn from_degrees(lat: f64, lon: f64) -> Option<Self> {
        let valid =
            lat.is_finite() && lon.is_finite() && (-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon);
        #[expect(
            clippy::cast_possible_truncation,
            reason = "the range check above keeps both values far inside i32"
        )]
        valid.then(|| Self {
            lat: (lat * MICRO).round() as i32,
            lon: (lon * MICRO).round() as i32,
        })
    }

    /// Latitude and longitude in degrees.
    pub fn degrees(self) -> (f64, f64) {
        (f64::from(self.lat) / MICRO, f64::from(self.lon) / MICRO)
    }
}

impl Server {
    /// Display name in `language`.
    pub fn name(&self, language: Language) -> String {
        match &self.label {
            ServerLabel::Automatic => i18n::text(language, Key::ServerAutomatic).to_owned(),
            ServerLabel::Place(original) => i18n::place(language, original),
            ServerLabel::Free => i18n::text(language, Key::ServerFree).to_owned(),
            ServerLabel::Plain => i18n::text(language, Key::ServerPlain).to_owned(),
        }
    }

    /// City in `language`.
    pub fn city(&self, language: Language) -> Option<String> {
        self.city.as_deref().map(|city| i18n::place(language, city))
    }
}

/// All servers of a country.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Country {
    /// Country code from `geo.countryCode`, for example "DE".
    pub code: String,
    /// English country name from the API; fallback for unknown codes and for the search.
    pub original: String,
    pub servers: Vec<Server>,
}

impl Country {
    /// Country name in `language`; for an unknown code the name from the API.
    pub fn name(&self, language: Language) -> String {
        i18n::country(language, &self.code).unwrap_or(&self.original).to_owned()
    }
}

/// Sorts by the country name in `language` (umlauts like their base letters).
pub fn sort_by_name(countries: &mut [Country], language: Language) {
    countries.sort_by_key(|country| names::sort_key(&country.name(language)));
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawEntry {
    hostname: Option<String>,
    display_name: Option<String>,
    geo: Option<RawGeo>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    children: Vec<RawEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawGeo {
    city_name: Option<String>,
    country_code: Option<String>,
    /// Read leniently: a value that is not a number makes only the position unknown, never the whole list
    /// unreadable.
    lat: Option<serde_json::Value>,
    lon: Option<serde_json::Value>,
    /// Read leniently as well: only a text with a known code counts.
    continent: Option<serde_json::Value>,
}

/// Name from the list for the display: without control characters, at most [`MAX_NAME`] characters;
/// empty counts as missing.
fn non_empty(name: Option<&String>) -> Option<String> {
    name.map(|name| text::printable(name, MAX_NAME))
        .filter(|name| !name.is_empty())
}

impl RawEntry {
    /// Country code in upper case; only exactly two letters count, otherwise the entry is discarded.
    fn country_code(&self) -> Option<String> {
        let code = self.geo.as_ref()?.country_code.as_ref()?.trim();
        (code.len() == 2 && code.bytes().all(|byte| byte.is_ascii_alphabetic())).then(|| code.to_ascii_uppercase())
    }

    /// Converts the entry into a server, or `None` if details are missing or the host name does not
    /// fit. A discarded entry does not disturb the others.
    ///
    /// A top-level entry carries the name of its country, which is already in the heading. If it has
    /// places below it, it is called "Automatic", otherwise it carries its city. Places below show
    /// their own name, translated when shown.
    fn server(&self, top_level: bool) -> Option<Server> {
        let original = non_empty(self.display_name.as_ref())?;
        let city = non_empty(self.geo.as_ref().and_then(|geo| geo.city_name.as_ref()));
        let free = self.tags.iter().any(|tag| tag == "free");
        let no_best = self.tags.iter().any(|tag| tag == "no-best");
        let position = self
            .geo
            .as_ref()
            .and_then(|geo| Position::from_degrees(geo.lat.as_ref()?.as_f64()?, geo.lon.as_ref()?.as_f64()?));
        let continent = self
            .geo
            .as_ref()
            .and_then(|geo| geo.continent.as_ref()?.as_str())
            .and_then(Continent::from_api);
        let automatic = top_level && !self.children.is_empty();
        let label = if automatic {
            ServerLabel::Automatic
        } else if top_level {
            match &city {
                Some(city) => ServerLabel::Place(city.clone()),
                None if free => ServerLabel::Free,
                None => ServerLabel::Plain,
            }
        } else {
            ServerLabel::Place(original.clone())
        };
        Some(Server {
            label,
            original,
            city,
            host: service_host(self.hostname.as_deref()?)?,
            free,
            automatic,
            derived: false,
            position,
            no_best,
            continent,
        })
    }
}

/// Error while evaluating the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidList;

/// Evaluates the list from the API: grouped by `geo.countryCode`, in the order of the API (sorting
/// happens when showing, by the language in force, [`sort_by_name`]). `children` count as further
/// servers of the same country. A host name appears only once per country; if a main entry names the
/// same server as a place below it, the place counts.
pub fn parse_list(content: &[u8]) -> Result<Vec<Country>, InvalidList> {
    let entries: Vec<RawEntry> = serde_json::from_slice(content).map_err(|_| InvalidList)?;
    let mut countries: Vec<Country> = Vec::new();
    for entry in &entries {
        let Some(code) = entry.country_code() else { continue };
        let children: Vec<Server> = entry.children.iter().filter_map(|child| child.server(false)).collect();
        let main = entry
            .server(true)
            .filter(|main| !children.iter().any(|child| child.host == main.host));
        let index = match countries.iter().position(|country| country.code == code) {
            Some(index) => index,
            None => {
                // The English country name is the display name of the first entry, without " - Free".
                let Some(original) = non_empty(entry.display_name.as_ref()) else {
                    continue;
                };
                let original = original.strip_suffix(" - Free").unwrap_or(&original).to_owned();
                countries.push(Country {
                    code,
                    original,
                    servers: Vec::new(),
                });
                countries.len() - 1
            }
        };
        let servers = &mut countries[index].servers;
        for server in main.into_iter().chain(children) {
            if !servers.iter().any(|known| known.host == server.host) {
                servers.push(server);
            }
        }
    }
    countries.retain(|country| !country.servers.is_empty());
    add_derived(&mut countries);
    Ok(countries)
}

/// Country name of hide.me for a country code in the form of the service: the code in lower case with
/// `.hideservers.net`, like the main entries of the list after [`service_host`] (`de-v4.hideservers.net` →
/// `de.hideservers.net`). The only exception is the United Kingdom, which hide.me calls `uk`. `None` if the
/// code does not give a permitted host name.
pub fn country_host(code: &str) -> Option<String> {
    if code.len() != 2 || !code.bytes().all(|byte| byte.is_ascii_alphabetic()) {
        return None;
    }
    let short = match code.to_ascii_lowercase().as_str() {
        "gb" => "uk".to_owned(),
        other => other.to_owned(),
    };
    let host = format!("{short}{HOST_SUFFIX}");
    is_valid_host(&host).then_some(host)
}

/// Adds the country name built by [`country_host`] at the front of every country with several servers
/// and without a main entry, so that the country row stands for "Automatic" like everywhere else (on
/// 2026-11-19 only Finland, whose main entry names the same server as "Helsinki #2"). Not if the name is
/// already a server of the country. The name is not queried at startup; a failed attempt removes it again
/// ([`drop_derived`]).
fn add_derived(countries: &mut [Country]) {
    for country in countries {
        if country.servers.len() < 2 || country.servers.iter().any(|server| server.automatic) {
            continue;
        }
        let Some(host) = country_host(&country.code) else {
            continue;
        };
        if country.servers.iter().any(|server| server.host == host) {
            continue;
        }
        country.servers.insert(
            0,
            Server {
                label: ServerLabel::Automatic,
                original: country.original.clone(),
                city: None,
                host,
                free: false,
                automatic: true,
                derived: true,
                position: None,
                no_best: false,
                continent: None,
            },
        );
    }
}

/// Removes built country names whose host is in `failed`: hide.me did not know them during this session.
/// Servers from the list stay, even if their host is in `failed`.
pub fn drop_derived(countries: &mut [Country], failed: &BTreeSet<String>) {
    for country in countries {
        country
            .servers
            .retain(|server| !(server.derived && failed.contains(&server.host)));
    }
}

/// Is `host` a country name built by Celata in `countries`?
pub fn is_derived(countries: &[Country], host: &str) -> bool {
    countries
        .iter()
        .flat_map(|country| &country.servers)
        .any(|server| server.derived && server.host == host)
}

/// A country found by [`search_marked`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// The country with the servers that matched; all of them if the country name matched.
    pub country: Country,
    /// The country name matched (otherwise only places or servers did).
    pub by_country: bool,
}

/// Searches case-insensitively in the names of all languages and in the name from the API. If the
/// country name matches, the whole country stays, otherwise only the servers whose name or city
/// matches. An empty search term shows everything.
pub fn search(countries: &[Country], query: &str) -> Vec<Country> {
    search_marked(countries, query)
        .into_iter()
        .map(|found| found.country)
        .collect()
}

/// Like [`search`], but remembers for every country whether its name matched. An empty search term
/// shows everything, marked as matched by the country name (nothing is unfolded by the search then).
pub fn search_marked(countries: &[Country], query: &str) -> Vec<Found> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return countries
            .iter()
            .map(|country| Found {
                country: country.clone(),
                by_country: true,
            })
            .collect();
    }
    let matches = |text: &str| text.to_lowercase().contains(&query);
    countries
        .iter()
        .filter_map(|country| {
            if matches(&country.original)
                || Language::ALL
                    .into_iter()
                    .any(|language| matches(&country.name(language)))
            {
                return Some(Found {
                    country: country.clone(),
                    by_country: true,
                });
            }
            let servers: Vec<Server> = country
                .servers
                .iter()
                .filter(|server| {
                    matches(&server.original)
                        || Language::ALL.into_iter().any(|language| {
                            matches(&server.name(language)) || server.city(language).as_deref().is_some_and(matches)
                        })
                })
                .cloned()
                .collect();
            (!servers.is_empty()).then(|| Found {
                country: Country {
                    servers,
                    ..country.clone()
                },
                by_country: false,
            })
        })
        .collect()
}

// --- Loading and cache ---

/// Error while loading the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    /// The API is not reachable, for example while the internet is blocked.
    Network,
    /// The response is unreadable or too large.
    Invalid,
}

impl LoadError {
    /// Message for the display, translated only when it is shown.
    pub fn phrase(&self) -> Phrase {
        Phrase::key(match self {
            Self::Network => Key::ServerListNetwork,
            Self::Invalid => Key::ServerListInvalid,
        })
    }
}

/// For debugging and `std::error::Error` only; displays use [`LoadError::phrase`].
impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.phrase().render(Language::En))
    }
}

impl std::error::Error for LoadError {}

/// Origin of a loaded list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Just loaded from the API.
    Fresh,
    /// From the cache, younger than 4 hours.
    Cached,
    /// From the cache, older than 4 hours, because loading failed.
    Stale,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loaded {
    pub countries: Vec<Country>,
    pub source: Source,
}

/// Content of the cache file: time of fetching and the public raw data.
#[derive(Serialize, Deserialize)]
struct CacheFile {
    fetched_at: u64,
    data: serde_json::Value,
}

/// Path of the cache file: `$XDG_CACHE_HOME/celata/servers.json`, alternatively `~/.cache`.
pub fn cache_path() -> Option<PathBuf> {
    Some(
        files::user_dir("XDG_CACHE_HOME", ".cache")?
            .join("celata")
            .join("servers.json"),
    )
}

/// Reads the cache. A missing or damaged file yields `None`.
fn read_cache(path: &Path) -> Option<(Vec<Country>, u64)> {
    let mut content = Vec::new();
    fs::File::open(path)
        .ok()?
        .take(MAX_LIST_SIZE * 2)
        .read_to_end(&mut content)
        .ok()?;
    let file: CacheFile = serde_json::from_slice(&content).ok()?;
    let raw = serde_json::to_vec(&file.data).ok()?;
    let countries = parse_list(&raw).ok().filter(|countries| !countries.is_empty())?;
    Some((countries, file.fetched_at))
}

/// Writes the cache through a temporary file, so that half a file never exists.
fn write_cache(path: &Path, content: &[u8], now: SystemTime) -> io::Result<()> {
    let data: serde_json::Value = serde_json::from_slice(content).map_err(io::Error::other)?;
    let file = CacheFile {
        fetched_at: unix_seconds(now),
        data,
    };
    files::write_atomic(
        path,
        &serde_json::to_vec(&file).map_err(io::Error::other)?,
        files::PUBLIC_MODE,
    )
}

/// Loads the list. A cache younger than 4 hours is used without fetching. If fetching fails, an
/// older list is used as well. `fetch` returns the raw data.
pub fn load<F>(cache: Option<&Path>, now: SystemTime, fetch: F) -> Result<Loaded, LoadError>
where
    F: FnOnce() -> Result<Vec<u8>, LoadError>,
{
    let cached = cache.and_then(read_cache);
    if let Some((countries, fetched_at)) = &cached {
        // If the time of fetching lies in the future, the clock is wrong: load anew then.
        let age = unix_seconds(now).checked_sub(*fetched_at).map(Duration::from_secs);
        if age.is_some_and(|age| age < CACHE_MAX_AGE) {
            return Ok(Loaded {
                countries: countries.clone(),
                source: Source::Cached,
            });
        }
    }
    let fetched = fetch().and_then(|content| {
        let countries = parse_list(&content).map_err(|_| LoadError::Invalid)?;
        if countries.is_empty() {
            return Err(LoadError::Invalid);
        }
        Ok((countries, content))
    });
    match fetched {
        Ok((countries, content)) => {
            if let Some(path) = cache {
                // A cache that cannot be written does not prevent the display.
                let _ = write_cache(path, &content, now);
            }
            Ok(Loaded {
                countries,
                source: Source::Fresh,
            })
        }
        Err(error) => match cached {
            Some((countries, _)) => Ok(Loaded {
                countries,
                source: Source::Stale,
            }),
            None => Err(error),
        },
    }
}

/// Fetches the public list over HTTPS.
pub fn fetch_list() -> Result<Vec<u8>, LoadError> {
    net::get(LIST_URL, MAX_LIST_SIZE).map_err(|error| match error {
        FetchError::TooLarge => LoadError::Invalid,
        FetchError::Network | FetchError::NotHttps => LoadError::Network,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::UNIX_EPOCH;

    const FIXTURE: &str = include_str!("../tests/fixtures/servers.json");
    /// The real list of 2026-11-19 (public, without access data).
    const LIST_2026_11_19: &[u8] = include_bytes!("../tests/fixtures/servers-2026-11-19.json");

    /// Names from the public list are shown without control characters and bounded; a country code that
    /// is not two letters discards the entry.
    #[test]
    fn namen_aus_der_liste_werden_bereinigt() {
        let long = "x".repeat(500);
        let list = format!(
            r#"[
              {{"hostname":"aa-v4.hideservers.net","displayName":"Evil\nLand\u001b[31m","geo":{{"cityName":"Ci\tty {long}","countryCode":"nl"}}}},
              {{"hostname":"bb-v4.hideservers.net","displayName":"Too long code","geo":{{"countryCode":"NLD"}}}},
              {{"hostname":"cc-v4.hideservers.net","displayName":"Digits","geo":{{"countryCode":"N1"}}}},
              {{"hostname":"dd-v4.hideservers.net","displayName":"{long}","geo":{{"countryCode":"DE"}}}}
            ]"#
        );
        let countries = parse_list(list.as_bytes()).unwrap();
        assert_eq!(
            countries
                .iter()
                .map(|country| country.code.as_str())
                .collect::<Vec<_>>(),
            ["NL", "DE"]
        );
        assert_eq!(countries[0].original, "EvilLand[31m");
        let city = countries[0].servers[0].city.clone().unwrap();
        assert!(city.starts_with("City xxx"), "{city}");
        assert_eq!(city.chars().count(), MAX_NAME);
        assert_eq!(countries[1].original.chars().count(), MAX_NAME);
        assert_eq!(countries[1].servers[0].original.chars().count(), MAX_NAME);
    }

    /// Sample list, sorted by the German country name as in the German display.
    fn fixture() -> Vec<Country> {
        let mut countries = parse_list(FIXTURE.as_bytes()).unwrap();
        sort_by_name(&mut countries, Language::De);
        countries
    }

    fn country<'a>(countries: &'a [Country], code: &str) -> &'a Country {
        countries.iter().find(|country| country.code == code).unwrap()
    }

    fn hosts(country: &Country) -> Vec<&str> {
        country.servers.iter().map(|server| server.host.as_str()).collect()
    }

    #[test]
    fn gueltige_hostnamen() {
        assert!(is_valid_host("nl.hideservers.net"));
        assert!(is_valid_host("free-us.hideservers.net"));
        assert!(is_valid_host("a1.hideservers.net"));
    }

    #[test]
    fn ungueltige_hostnamen() {
        for host in [
            "",
            ".hideservers.net",
            "nl",
            "nl.hideservers.net.example.org",
            "NL.hideservers.net",
            "a.b.hideservers.net",
            "nl_x.hideservers.net",
            "nl\".hideservers.net",
        ] {
            assert!(!is_valid_host(host), "{host}");
        }
    }

    #[test]
    fn hostname_wie_bei_hide_me_list() {
        assert_eq!(
            service_host("nl-v4.hideservers.net").as_deref(),
            Some("nl.hideservers.net")
        );
        assert_eq!(
            service_host("free-de-v4.hideservers.net").as_deref(),
            Some("free-de.hideservers.net")
        );
        assert_eq!(
            service_host("fi2-v4.hideservers.net").as_deref(),
            Some("fi2.hideservers.net")
        );
        assert_eq!(
            service_host("nl.hideservers.net").as_deref(),
            Some("nl.hideservers.net")
        );
        assert_eq!(service_host("nl-v4.example.org"), None);
        assert_eq!(service_host("x\",\"Password\":\"y-v4.hideservers.net"), None);
    }

    #[test]
    fn gruppiert_nach_land_und_sortiert_nach_namen() {
        let countries = fixture();
        let names: Vec<_> = countries.iter().map(|country| country.name(Language::De)).collect();
        assert_eq!(names, vec!["Australien", "Deutschland", "Neuseeland", "Niederlande"]);
        // In English: "Netherlands" before "New Zealand".
        let mut english = countries.clone();
        sort_by_name(&mut english, Language::En);
        let names: Vec<_> = english.iter().map(|country| country.name(Language::En)).collect();
        assert_eq!(names, vec!["Australia", "Germany", "Netherlands", "New Zealand"]);
        let originals: Vec<_> = countries.iter().map(|country| country.original.as_str()).collect();
        assert_eq!(originals, vec!["Australia", "Germany", "New Zealand", "Netherlands"]);
    }

    fn labels(country: &Country) -> Vec<String> {
        country.servers.iter().map(|server| server.name(Language::De)).collect()
    }

    #[test]
    fn land_erscheint_nicht_noch_einmal_als_server() {
        let countries = fixture();
        // Main entry with places below it: hide.me chooses the place.
        assert_eq!(
            labels(country(&countries, "AU")),
            vec!["Automatisch", "Melbourne", "Perth"]
        );
        assert!(country(&countries, "AU").servers[0].automatic);
        assert!(!country(&countries, "AU").servers[1].automatic);
        // Only server of a country: the city instead of the country name.
        assert_eq!(labels(country(&countries, "NZ")), vec!["Auckland"]);
        assert!(!country(&countries, "NZ").servers[0].automatic);
        // Free server: the city, marked through `free`.
        assert_eq!(
            labels(country(&countries, "DE")),
            vec!["Automatisch", "Frankfurt", "Berlin", "Frankfurt"]
        );
        for country in &countries {
            let own = country.name(Language::De);
            assert!(
                country
                    .servers
                    .iter()
                    .all(|server| server.name(Language::De) != own && server.name(Language::De) != country.original)
            );
        }
    }

    #[test]
    fn haupteintrag_gleich_einem_unterort_erscheint_einmal() {
        // Like Finland in the real list: the main entry names the same server as "Helsinki #2".
        let list = br#"[
          {"hostname": "fi2-v4.hideservers.net", "displayName": "Finland", "geo": {"countryCode": "FI"},
           "children": [
             {"hostname": "fi1-v4.hideservers.net", "displayName": "Helsinki #1", "geo": {"cityName": "Helsinki", "countryCode": "FI"}},
             {"hostname": "fi2-v4.hideservers.net", "displayName": "Helsinki #2", "geo": {"cityName": "Helsinki", "countryCode": "FI"}}
           ]},
          {"hostname": "free-ch-v4.hideservers.net", "displayName": "Switzerland - Free", "geo": {"countryCode": "CH"}, "tags": ["free"]},
          {"hostname": "at-v4.hideservers.net", "displayName": "Austria", "geo": {"cityName": "Vienna", "countryCode": "AT"}},
          {"hostname": "it-v4.hideservers.net", "displayName": "Italy", "geo": {"countryCode": "IT"},
           "children": [{"hostname": "milan-v4.hideservers.net", "displayName": "Milan", "geo": {"cityName": "Milan", "countryCode": "IT"}}]}
        ]"#;
        let mut countries = parse_list(list).unwrap();
        sort_by_name(&mut countries, Language::De);
        let names: Vec<_> = countries.iter().map(|country| country.name(Language::De)).collect();
        assert_eq!(names, vec!["Finnland", "Italien", "Österreich", "Schweiz"]);
        // Celata builds the missing country name (feature 012).
        assert_eq!(labels(&countries[0]), vec!["Automatisch", "Helsinki #1", "Helsinki #2"]);
        assert!(countries[0].servers[0].derived);
        assert_eq!(labels(&countries[1]), vec!["Automatisch", "Mailand"]);
        assert_eq!(labels(&countries[2]), vec!["Wien"]);
        assert_eq!(labels(&countries[3]), vec!["Kostenloser Server"]);
        // In English the name from the API or the English texts.
        let english: Vec<_> = countries[1]
            .servers
            .iter()
            .map(|server| server.name(Language::En))
            .collect();
        assert_eq!(english, vec!["Automatic", "Milan"]);
        assert_eq!(countries[3].servers[0].name(Language::En), "Free server");
        assert_eq!(countries[2].servers[0].city(Language::De).as_deref(), Some("Wien"));
        assert_eq!(countries[2].servers[0].city(Language::En).as_deref(), Some("Vienna"));
        assert!(!countries[3].servers[0].automatic);
    }

    #[test]
    fn unterorte_und_freie_server_gehoeren_zum_land() {
        let countries = fixture();
        assert_eq!(
            hosts(country(&countries, "DE")),
            vec![
                "de.hideservers.net",
                "frankfurt.hideservers.net",
                "berlin.hideservers.net",
                "free-de.hideservers.net"
            ]
        );
        let germany = country(&countries, "DE");
        assert_eq!(germany.name(Language::De), "Deutschland");
        assert_eq!(germany.name(Language::En), "Germany");
        assert!(
            germany
                .servers
                .iter()
                .filter(|server| server.free)
                .map(|server| &server.original)
                .eq(["Germany - Free"])
        );
    }

    #[test]
    fn stadtname_wird_uebernommen() {
        let countries = fixture();
        let berlin = country(&countries, "DE")
            .servers
            .iter()
            .find(|server| server.name(Language::De) == "Berlin")
            .unwrap();
        assert_eq!(berlin.city.as_deref(), Some("Berlin"));
        assert_eq!(berlin.original, "Berlin");
        assert!(!berlin.free);
        assert_eq!(country(&countries, "AU").servers[0].city, None);
    }

    #[test]
    fn ungueltige_eintraege_werden_verworfen() {
        let countries = fixture();
        // The sample contains a server with a foreign domain and one without a display name.
        let all: Vec<_> = countries.iter().flat_map(hosts).collect();
        assert!(all.iter().all(|host| is_valid_host(host)));
        assert!(!all.iter().any(|host| host.contains("example")));
        assert_eq!(hosts(country(&countries, "NL")), vec!["nl.hideservers.net"]);
    }

    #[test]
    fn unlesbare_liste_ist_ein_fehler() {
        assert_eq!(parse_list(b"kein json"), Err(InvalidList));
        assert_eq!(parse_list(br#"{"result":[]}"#), Err(InvalidList));
        assert_eq!(parse_list(b"[]"), Ok(vec![]));
    }

    fn found(query: &str) -> Vec<(String, Vec<String>)> {
        search(&fixture(), query)
            .into_iter()
            .map(|country| {
                (
                    country.name(Language::De),
                    country.servers.iter().map(|server| server.name(Language::De)).collect(),
                )
            })
            .collect()
    }

    #[test]
    fn suche_nach_land_zeigt_alle_server_des_landes() {
        for query in ["germany", "deutsch", "GERMANY", "Deutschland"] {
            let result = found(query);
            assert_eq!(result.len(), 1, "{query}");
            assert_eq!(result[0].1.len(), 4, "{query}");
        }
    }

    #[test]
    fn suche_nach_ort_ohne_gross_und_kleinschreibung() {
        assert_eq!(
            found("MELBOURNE"),
            vec![("Australien".to_owned(), vec!["Melbourne".to_owned()])]
        );
        assert_eq!(
            found("Amsterdam"),
            vec![("Niederlande".to_owned(), vec!["Amsterdam".to_owned()])]
        );
    }

    #[test]
    fn suche_teilwort_und_leerzeichen() {
        assert_eq!(found("  free ").len(), 1);
        assert_eq!(found("new").len(), 1);
        assert_eq!(found("automatisch").len(), 2);
        assert_eq!(found("automatic").len(), 2);
    }

    #[test]
    fn leere_suche_zeigt_alles_und_keine_treffer_ist_leer() {
        assert_eq!(search(&fixture(), "").len(), 4);
        assert!(found("Atlantis").is_empty());
    }

    // --- Cache ---

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn cache_file() -> PathBuf {
        let number = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir()
            .join(format!("celata-cache-{}-{number}", std::process::id()))
            .join("celata")
            .join("servers.json")
    }

    fn at(hours: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_800_000_000 + hours * 3600)
    }

    fn never() -> Result<Vec<u8>, LoadError> {
        panic!("Es hätte nicht geladen werden dürfen");
    }

    #[expect(
        clippy::unnecessary_wraps,
        reason = "has the signature of the fetch function it stands in for"
    )]
    fn fixture_bytes() -> Result<Vec<u8>, LoadError> {
        Ok(FIXTURE.as_bytes().to_vec())
    }

    #[test]
    fn laedt_und_legt_zwischenspeicher_an() {
        let path = cache_file();
        let loaded = load(Some(&path), at(0), fixture_bytes).unwrap();
        assert_eq!(loaded.source, Source::Fresh);
        assert_eq!(loaded.countries, parse_list(FIXTURE.as_bytes()).unwrap());
        assert!(path.exists());
    }

    #[test]
    fn junger_zwischenspeicher_wird_ohne_abruf_genutzt() {
        let path = cache_file();
        load(Some(&path), at(0), fixture_bytes).unwrap();
        let loaded = load(Some(&path), at(3), never).unwrap();
        assert_eq!(loaded.source, Source::Cached);
        assert_eq!(loaded.countries, parse_list(FIXTURE.as_bytes()).unwrap());
    }

    #[test]
    fn alter_zwischenspeicher_wird_erneuert() {
        let path = cache_file();
        load(Some(&path), at(0), fixture_bytes).unwrap();
        let mut fetched = false;
        let loaded = load(Some(&path), at(5), || {
            fetched = true;
            fixture_bytes()
        })
        .unwrap();
        assert!(fetched);
        assert_eq!(loaded.source, Source::Fresh);
        // The new time of fetching applies: one hour later nothing is loaded again.
        assert_eq!(load(Some(&path), at(6), never).unwrap().source, Source::Cached);
    }

    #[test]
    fn scheitert_der_abruf_wird_die_alte_liste_genutzt() {
        let path = cache_file();
        load(Some(&path), at(0), fixture_bytes).unwrap();
        let loaded = load(Some(&path), at(48), || Err(LoadError::Network)).unwrap();
        assert_eq!(loaded.source, Source::Stale);
        assert_eq!(loaded.countries, parse_list(FIXTURE.as_bytes()).unwrap());
        // An unreadable response does not replace the old list either.
        let loaded = load(Some(&path), at(48), || Ok(b"kaputt".to_vec())).unwrap();
        assert_eq!(loaded.source, Source::Stale);
    }

    #[test]
    fn ohne_zwischenspeicher_wird_der_fehler_gemeldet() {
        assert_eq!(
            load(Some(&cache_file()), at(0), || Err(LoadError::Network)),
            Err(LoadError::Network)
        );
        assert_eq!(load(None, at(0), || Ok(b"[]".to_vec())), Err(LoadError::Invalid));
    }

    #[test]
    fn beschaedigte_datei_wird_ignoriert() {
        let path = cache_file();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"{ halb geschrieben").unwrap();
        let loaded = load(Some(&path), at(0), fixture_bytes).unwrap();
        assert_eq!(loaded.source, Source::Fresh);
        // The file is in order again afterwards.
        assert_eq!(load(Some(&path), at(1), never).unwrap().source, Source::Cached);
    }

    #[test]
    fn abrufzeit_in_der_zukunft_fuehrt_zum_neuen_abruf() {
        let path = cache_file();
        load(Some(&path), at(10), fixture_bytes).unwrap();
        assert_eq!(load(Some(&path), at(0), fixture_bytes).unwrap().source, Source::Fresh);
    }

    #[test]
    fn fehlermeldungen_nennen_einen_naechsten_schritt() {
        assert!(
            LoadError::Network
                .phrase()
                .render(Language::De)
                .contains("versuche es erneut")
        );
        assert!(
            LoadError::Invalid
                .phrase()
                .render(Language::De)
                .contains("später erneut")
        );
        assert!(LoadError::Network.phrase().render(Language::En).contains("try again"));
        assert!(
            LoadError::Invalid
                .phrase()
                .render(Language::En)
                .contains("Try again later")
        );
    }

    #[test]
    fn suche_findet_orte_in_beiden_sprachen() {
        let list = br#"[
          {"hostname": "at-v4.hideservers.net", "displayName": "Austria", "geo": {"cityName": "Vienna", "countryCode": "AT"}},
          {"hostname": "it-v4.hideservers.net", "displayName": "Italy", "geo": {"countryCode": "IT"},
           "children": [{"hostname": "milan-v4.hideservers.net", "displayName": "Milan", "geo": {"cityName": "Milan", "countryCode": "IT"}}]}
        ]"#;
        let countries = parse_list(list).unwrap();
        for query in ["Wien", "vienna", "Österreich", "austria"] {
            let result = search(&countries, query);
            assert_eq!(result.len(), 1, "{query}");
            assert_eq!(result[0].code, "AT", "{query}");
        }
        for query in ["Mailand", "milan"] {
            let result = search(&countries, query);
            assert_eq!(result.len(), 1, "{query}");
            assert_eq!(result[0].servers.len(), 1, "{query}");
            assert_eq!(result[0].servers[0].host, "milan.hideservers.net", "{query}");
        }
    }

    // --- Coordinates and flags (feature 013) ---

    fn server<'a>(countries: &'a [Country], host: &str) -> &'a Server {
        countries
            .iter()
            .flat_map(|country| &country.servers)
            .find(|server| server.host == host)
            .unwrap_or_else(|| panic!("{host} fehlt"))
    }

    #[test]
    fn koordinaten_aus_der_echten_liste() {
        let countries = real_list();
        let (lat, lon) = server(&countries, "berlin.hideservers.net").position.unwrap().degrees();
        assert!((lat - 52.52).abs() < 1e-6 && (lon - 13.405).abs() < 1e-6, "{lat} {lon}");
        // Country names ("Automatic") carry no coordinates in the list.
        assert_eq!(server(&countries, "de.hideservers.net").position, None);
        // Built country name.
        let finland = server(&countries, "fi.hideservers.net");
        assert!(finland.derived);
        assert_eq!(finland.position, None);
        assert!(!finland.no_best);
    }

    #[test]
    fn kennzeichen_no_best() {
        let countries = real_list();
        let free_fi = server(&countries, "free-fi.hideservers.net");
        assert!(free_fi.free && free_fi.no_best);
        assert_eq!(free_fi.name(Language::De), "Helsinki");
        let free_uk = server(&countries, "free-uk.hideservers.net");
        assert!(free_uk.free && free_uk.no_best);
        assert_eq!(free_uk.name(Language::De), "Kostenloser Server");
        assert!(!server(&countries, "berlin.hideservers.net").no_best);
        let flagged: Vec<&str> = countries
            .iter()
            .flat_map(|country| &country.servers)
            .filter(|server| server.no_best)
            .map(|server| server.host.as_str())
            .collect();
        assert_eq!(flagged, ["free-fi.hideservers.net", "free-uk.hideservers.net"]);
    }

    #[test]
    fn ungueltige_koordinaten_ergeben_keinen_ort() {
        let list = br#"[
          {"hostname": "a-v4.hideservers.net", "displayName": "A", "geo": {"countryCode": "AA", "lat": 91, "lon": 10}},
          {"hostname": "b-v4.hideservers.net", "displayName": "B", "geo": {"countryCode": "BB", "lat": 10, "lon": -181}},
          {"hostname": "c-v4.hideservers.net", "displayName": "C", "geo": {"countryCode": "CC", "lat": 10}},
          {"hostname": "d-v4.hideservers.net", "displayName": "D", "geo": {"countryCode": "DD", "lat": "NaN", "lon": 5}},
          {"hostname": "e-v4.hideservers.net", "displayName": "E", "geo": {"countryCode": "EE", "lat": 52.5000004, "lon": -0.0000006}},
          {"hostname": "f-v4.hideservers.net", "displayName": "F", "geo": {"countryCode": "FF", "lat": -90, "lon": 180}}
        ]"#;
        let countries = parse_list(list).unwrap();
        // Every server stays in the list.
        assert_eq!(countries.len(), 6);
        for host in ["a", "b", "c", "d"] {
            assert_eq!(
                server(&countries, &format!("{host}.hideservers.net")).position,
                None,
                "{host}"
            );
        }
        assert_eq!(
            server(&countries, "e.hideservers.net").position,
            Some(Position {
                lat: 52_500_000,
                lon: -1
            })
        );
        assert_eq!(
            server(&countries, "f.hideservers.net").position,
            Some(Position {
                lat: -90_000_000,
                lon: 180_000_000
            })
        );
        assert_eq!(Position::from_degrees(f64::NAN, 0.0), None);
        assert_eq!(Position::from_degrees(0.0, f64::INFINITY), None);
    }

    // --- Built country names (feature 012) ---

    fn real_list() -> Vec<Country> {
        parse_list(LIST_2026_11_19).unwrap()
    }

    #[test]
    fn laendername_nach_dem_muster_von_hide_me() {
        assert_eq!(country_host("FI").as_deref(), Some("fi.hideservers.net"));
        assert_eq!(country_host("GB").as_deref(), Some("uk.hideservers.net"));
        assert_eq!(country_host("DE").as_deref(), Some("de.hideservers.net"));
        assert_eq!(country_host("de").as_deref(), Some("de.hideservers.net"));
        for code in ["", "D", "DEU", "D1", "D-", "Ä"] {
            assert_eq!(country_host(code), None, "{code}");
        }
    }

    #[test]
    fn echte_liste_hat_57_laender_und_elf_mit_mehreren_orten() {
        let countries = real_list();
        assert_eq!(countries.len(), 57);
        let mut several: Vec<&str> = countries
            .iter()
            .filter(|country| country.servers.len() > 1)
            .map(|country| country.code.as_str())
            .collect();
        several.sort_unstable();
        assert_eq!(
            several,
            ["AU", "CA", "CH", "DE", "ES", "FI", "FR", "GB", "IT", "NL", "US"]
        );
        // Every country with several places has exactly one "Automatic" at the front.
        for country in countries.iter().filter(|country| country.servers.len() > 1) {
            assert!(country.servers[0].automatic, "{}", country.code);
            assert_eq!(
                country.servers.iter().filter(|server| server.automatic).count(),
                1,
                "{}",
                country.code
            );
        }
    }

    #[test]
    fn finnland_bekommt_den_gebildeten_laendernamen() {
        let countries = real_list();
        let finland = country(&countries, "FI");
        let first = &finland.servers[0];
        assert_eq!(first.host, "fi.hideservers.net");
        assert!(first.automatic && first.derived);
        assert_eq!(first.label, ServerLabel::Automatic);
        assert_eq!(
            hosts(finland),
            [
                "fi.hideservers.net",
                "fi1.hideservers.net",
                "fi2.hideservers.net",
                "free-fi.hideservers.net"
            ]
        );
        // Germany keeps its own main entry and gets no second one.
        let germany = country(&countries, "DE");
        let automatic: Vec<&Server> = germany.servers.iter().filter(|server| server.automatic).collect();
        assert_eq!(automatic.len(), 1);
        assert_eq!(automatic[0].host, "de.hideservers.net");
        assert!(!automatic[0].derived);
        // Only Finland has a built name.
        let derived: Vec<&str> = countries
            .iter()
            .flat_map(|country| &country.servers)
            .filter(|server| server.derived)
            .map(|server| server.host.as_str())
            .collect();
        assert_eq!(derived, ["fi.hideservers.net"]);
        assert!(is_derived(&countries, "fi.hideservers.net"));
        assert!(!is_derived(&countries, "de.hideservers.net"));
    }

    #[test]
    fn kein_gebildeter_name_bei_einem_server_oder_vorhandenem_namen() {
        let list = br#"[
          {"hostname": "at-v4.hideservers.net", "displayName": "Austria", "geo": {"cityName": "Vienna", "countryCode": "AT"}},
          {"hostname": "se1-v4.hideservers.net", "displayName": "Sweden", "geo": {"cityName": "Stockholm", "countryCode": "SE"}},
          {"hostname": "se-v4.hideservers.net", "displayName": "Sweden", "geo": {"cityName": "Malmo", "countryCode": "SE"}}
        ]"#;
        let countries = parse_list(list).unwrap();
        assert!(
            countries
                .iter()
                .flat_map(|country| &country.servers)
                .all(|server| !server.derived)
        );
        assert_eq!(
            hosts(country(&countries, "SE")),
            ["se1.hideservers.net", "se.hideservers.net"]
        );
    }

    #[test]
    fn gescheiterter_gebildeter_name_faellt_weg() {
        let mut countries = real_list();
        let failed: BTreeSet<String> = ["fi.hideservers.net".to_owned(), "de.hideservers.net".to_owned()].into();
        drop_derived(&mut countries, &failed);
        assert_eq!(
            hosts(country(&countries, "FI")),
            ["fi1.hideservers.net", "fi2.hideservers.net", "free-fi.hideservers.net"]
        );
        // A real main entry stays, even if its host is in the set.
        assert_eq!(country(&countries, "DE").servers[0].host, "de.hideservers.net");
        assert!(!is_derived(&countries, "fi.hideservers.net"));
    }

    #[test]
    fn markierte_suche_nach_land_und_ort() {
        let countries = real_list();
        for query in ["deutsch", "germany"] {
            let found = search_marked(&countries, query);
            assert_eq!(found.len(), 1, "{query}");
            assert_eq!(found[0].country.code, "DE");
            assert!(found[0].by_country, "{query}");
            assert_eq!(found[0].country.servers, country(&countries, "DE").servers);
        }
        // "frank" also matches the country name "Frankreich" (France): France whole, Germany only Frankfurt.
        let found = search_marked(&countries, "frank");
        let codes: Vec<&str> = found.iter().map(|found| found.country.code.as_str()).collect();
        assert_eq!(codes.len(), 2, "{codes:?}");
        let france = found.iter().find(|found| found.country.code == "FR").unwrap();
        assert!(france.by_country);
        let germany = found.iter().find(|found| found.country.code == "DE").unwrap();
        assert!(!germany.by_country);
        assert_eq!(
            hosts(&germany.country),
            [
                "frankfurt1.hideservers.net",
                "frankfurt2.hideservers.net",
                "free-de.hideservers.net"
            ]
        );
        // `search` gives the same countries and servers as before.
        for query in ["", "frank", "deutsch", "helsinki", "automatic", "Atlantis"] {
            let plain = search(&countries, query);
            let marked: Vec<Country> = search_marked(&countries, query)
                .into_iter()
                .map(|found| found.country)
                .collect();
            assert_eq!(plain, marked, "{query}");
        }
    }

    // --- Continent from the list (feature 016) ---

    #[test]
    fn continent_is_read_from_geo() {
        let list = br#"[
          {"hostname": "a-v4.hideservers.net", "displayName": "A", "geo": {"countryCode": "AA", "continent": "EU"}},
          {"hostname": "b-v4.hideservers.net", "displayName": "B", "geo": {"countryCode": "BB", "continent": " oc "}},
          {"hostname": "c-v4.hideservers.net", "displayName": "C", "geo": {"countryCode": "CC"}},
          {"hostname": "d-v4.hideservers.net", "displayName": "D", "geo": {"countryCode": "DD", "continent": 5}},
          {"hostname": "e-v4.hideservers.net", "displayName": "E", "geo": {"countryCode": "EE", "continent": {"x": 1}}},
          {"hostname": "f-v4.hideservers.net", "displayName": "F", "geo": {"countryCode": "FF", "continent": "AN"}}
        ]"#;
        let countries = parse_list(list).unwrap();
        // Every server stays in the list, whatever the value looks like.
        assert_eq!(countries.len(), 6);
        assert_eq!(
            server(&countries, "a.hideservers.net").continent,
            Some(Continent::Europe)
        );
        assert_eq!(
            server(&countries, "b.hideservers.net").continent,
            Some(Continent::Oceania)
        );
        for host in ["c", "d", "e", "f"] {
            assert_eq!(
                server(&countries, &format!("{host}.hideservers.net")).continent,
                None,
                "{host}"
            );
        }
    }

    #[test]
    fn every_server_of_the_real_list_has_a_continent_except_built_names() {
        let countries = real_list();
        let mut places = 0;
        for server in countries.iter().flat_map(|country| &country.servers) {
            if server.derived {
                assert_eq!(server.continent, None, "{}", server.host);
            } else {
                assert!(server.continent.is_some(), "{}", server.host);
                places += 1;
            }
        }
        // The list has 104 servers (the entry of Finland and its place "Helsinki #2" name the same host).
        assert_eq!(places, 104);
    }
}
