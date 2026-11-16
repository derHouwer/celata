// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! Filter categories of hide.me.
//!
//! The list can be fetched publicly without signing in. Names are sent exactly as hide.me delivers them;
//! the English descriptions are only displayed.

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::files::{self, unix_seconds};
use crate::i18n::{Key, Language, Phrase};
use crate::net::{self, FetchError};
use crate::text;

/// Public list of the categories.
pub const LIST_URL: &str = "https://nl.hideservers.net/categorization/categories.json";
/// Largest permitted list.
pub const MAX_LIST_SIZE: u64 = 256 * 1024;
/// The cache is valid for this long.
pub const CACHE_MAX_AGE: Duration = Duration::from_hours(7 * 24);
const MAX_DESCRIPTION: usize = 1024;

/// A category from the list of hide.me.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Category {
    pub name: String,
    pub description: String,
}

/// Error while loading the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadError {
    Network,
    Invalid,
}

impl LoadError {
    /// Message for the display, translated only when it is shown.
    pub fn phrase(&self) -> Phrase {
        Phrase::key(match self {
            Self::Network => Key::CategoriesNetwork,
            Self::Invalid => Key::CategoriesInvalid,
        })
    }
}

/// For debugging only; displays use [`LoadError::phrase`].
impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.phrase().render(Language::En))
    }
}

/// Name: 1–64 characters from lower-case letters, digits, spaces and `- & / ( ) , '`.
///
/// Brackets, comma and apostrophe occur in 12 of the 141 real names (for example "finance (general)",
/// "kid's pages", fetched 2026-11-09); without them these categories would be missing.
pub fn is_valid_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b' ' | b'-' | b'&' | b'/' | b'(' | b')' | b',' | b'\'')
        })
}

#[derive(Deserialize)]
struct RawCategory {
    name: Option<String>,
    description: Option<String>,
}

/// Reads the list. Invalid entries are discarded, duplicate names are taken only once.
pub fn parse(content: &[u8]) -> Result<Vec<Category>, LoadError> {
    if content.len() as u64 > MAX_LIST_SIZE {
        return Err(LoadError::Invalid);
    }
    let raw: Vec<RawCategory> = serde_json::from_slice(content).map_err(|_| LoadError::Invalid)?;
    let mut list: Vec<Category> = Vec::new();
    for entry in raw {
        let Some(name) = entry.name.filter(|name| is_valid_name(name)) else {
            continue;
        };
        if list.iter().any(|known| known.name == name) {
            continue;
        }
        let description = text::printable(&entry.description.unwrap_or_default(), MAX_DESCRIPTION);
        list.push(Category { name, description });
    }
    Ok(list)
}

/// The search shows at most this many hits, so that the page stays clear.
pub const SEARCH_LIMIT: usize = 8;

/// Finds categories whose name or description contains `query` (case-insensitive). Categories that are
/// already blocked (`chosen`) do not appear. Without a search term there are no hits.
pub fn search<'a>(list: &'a [Category], query: &str, chosen: &std::collections::BTreeSet<String>) -> Vec<&'a Category> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Vec::new();
    }
    let mut hits: Vec<&Category> = list
        .iter()
        .filter(|category| !chosen.contains(&category.name))
        .filter(|category| category.name.contains(&query) || category.description.to_lowercase().contains(&query))
        .collect();
    // Hits in the name first.
    hits.sort_by_key(|category| !category.name.contains(&query));
    hits.truncate(SEARCH_LIMIT);
    hits
}

/// `~/.cache/celata/categories.json`.
pub fn cache_path() -> Option<PathBuf> {
    crate::servers::cache_path().map(|path| path.with_file_name("categories.json"))
}

#[derive(Serialize, Deserialize)]
struct CacheFile {
    fetched_at: u64,
    categories: Vec<Category>,
}

fn read_cache(path: &Path) -> Option<(Vec<Category>, u64)> {
    let mut content = Vec::new();
    fs::File::open(path)
        .ok()?
        .take(MAX_LIST_SIZE * 2)
        .read_to_end(&mut content)
        .ok()?;
    let file: CacheFile = serde_json::from_slice(&content).ok()?;
    // Check the cache like the list as well.
    let raw = serde_json::to_vec(&file.categories).ok()?;
    let categories = parse(&raw).ok().filter(|list| !list.is_empty())?;
    Some((categories, file.fetched_at))
}

fn write_cache(path: &Path, categories: &[Category], now: SystemTime) -> io::Result<()> {
    let file = CacheFile {
        fetched_at: unix_seconds(now),
        categories: categories.to_vec(),
    };
    files::write_atomic(
        path,
        &serde_json::to_vec(&file).map_err(io::Error::other)?,
        files::PUBLIC_MODE,
    )
}

/// Loads the list: from the cache if it is younger than 7 days, otherwise from hide.me. If fetching
/// fails, an older cache is used as well.
pub fn load<F>(cache: Option<&Path>, now: SystemTime, fetch: F) -> Result<Vec<Category>, LoadError>
where
    F: FnOnce() -> Result<Vec<u8>, LoadError>,
{
    let cached = cache.and_then(read_cache);
    if let Some((categories, fetched_at)) = &cached {
        let age = unix_seconds(now).checked_sub(*fetched_at).map(Duration::from_secs);
        if age.is_some_and(|age| age < CACHE_MAX_AGE) {
            return Ok(categories.clone());
        }
    }
    let fetched = fetch().and_then(|content| {
        let categories = parse(&content)?;
        if categories.is_empty() {
            Err(LoadError::Invalid)
        } else {
            Ok(categories)
        }
    });
    match fetched {
        Ok(categories) => {
            if let Some(path) = cache {
                let _ = write_cache(path, &categories, now);
            }
            Ok(categories)
        }
        Err(error) => cached.map(|(categories, _)| categories).ok_or(error),
    }
}

/// Fetches the public list over HTTPS, without signing in.
pub fn fetch() -> Result<Vec<u8>, LoadError> {
    net::get(LIST_URL, MAX_LIST_SIZE).map_err(|error| match error {
        FetchError::TooLarge => LoadError::Invalid,
        FetchError::Network | FetchError::NotHttps => LoadError::Network,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn fixture() -> Vec<u8> {
        fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/categories.json")).unwrap()
    }

    fn temp_cache() -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let number = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("celata-kategorien-{}-{number}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir.join("categories.json")
    }

    #[test]
    fn ungueltige_eintraege_werden_verworfen() {
        let list = parse(&fixture()).unwrap();
        let names: Vec<&str> = list.iter().map(|category| category.name.as_str()).collect();
        assert_eq!(names, ["gambling", "hate speech", "finance (general)", "kid's pages"]);
        assert!(list[0].description.starts_with("Web pages"));
    }

    #[test]
    fn suche_nach_name_und_beschreibung() {
        let list = parse(&fixture()).unwrap();
        let names = |hits: Vec<&Category>| hits.into_iter().map(|c| c.name.clone()).collect::<Vec<_>>();
        assert!(search(&list, "  ", &BTreeSet::new()).is_empty());
        assert_eq!(names(search(&list, "GAMB", &BTreeSet::new())), ["gambling"]);
        assert_eq!(names(search(&list, "children", &BTreeSet::new())), ["kid's pages"]);
        let chosen = ["gambling".to_owned()].into();
        assert!(search(&list, "gamb", &chosen).is_empty());
        // Hits in the name before hits in the description.
        assert_eq!(
            names(search(&list, "finance", &BTreeSet::new()))[0],
            "finance (general)"
        );
    }

    #[test]
    fn namensregeln() {
        assert!(is_valid_name("web hosting, isp & telco"));
        assert!(is_valid_name("news/media"));
        assert!(!is_valid_name(""));
        assert!(!is_valid_name("Gambling"));
        assert!(!is_valid_name("a\"b"));
        assert!(!is_valid_name(&"a".repeat(65)));
    }

    #[test]
    fn zu_grosse_oder_kaputte_liste_ist_ungueltig() {
        assert_eq!(parse(&vec![b' '; 256 * 1024 + 1]), Err(LoadError::Invalid));
        assert_eq!(parse(b"{\"name\":1}"), Err(LoadError::Invalid));
    }

    #[test]
    fn frischer_zwischenspeicher_wird_ohne_abruf_genutzt() {
        let cache = temp_cache();
        let now = SystemTime::now();
        let first = load(Some(&cache), now, || Ok(fixture())).unwrap();
        let second = load(Some(&cache), now + Duration::from_secs(3600), || {
            panic!("kein Abruf erwartet")
        })
        .unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn alter_zwischenspeicher_hilft_wenn_der_abruf_scheitert() {
        let cache = temp_cache();
        let now = SystemTime::now();
        load(Some(&cache), now, || Ok(fixture())).unwrap();
        let later = now + CACHE_MAX_AGE + Duration::from_secs(1);
        assert_eq!(load(Some(&cache), later, || Err(LoadError::Network)).unwrap().len(), 4);
        assert_eq!(load(None, later, || Err(LoadError::Network)), Err(LoadError::Network));
    }
}
