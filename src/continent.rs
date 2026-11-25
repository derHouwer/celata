// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! Continents of the countries, for the location menu of the tray icon.
//!
//! Celata's own table decides first, so that a changed value in the server list never moves a known country
//! unnoticed. For a country that is not in the table, the list itself, the nearest country and in the end
//! Europe decide. Pure logic: no network, no files, nothing is stored.

use std::collections::BTreeMap;

use crate::best;
use crate::i18n::{self, Key, Language};
use crate::servers::{Country, Position};

/// A continent of the menu. The order of the declaration is the order in the menu, in every language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Continent {
    Europe,
    Asia,
    NorthAmerica,
    SouthAmerica,
    Africa,
    Oceania,
}

/// All continents in the order of the menu.
pub const ALL: [Continent; 6] = [
    Continent::Europe,
    Continent::Asia,
    Continent::NorthAmerica,
    Continent::SouthAmerica,
    Continent::Africa,
    Continent::Oceania,
];

use Continent::{Africa as AF, Asia as AS, Europe as EU, NorthAmerica as NA, Oceania as OC, SouthAmerica as SA};

/// Continent of every known country, in the order of `names::CODES`. The countries of the list of 2026-11-19
/// agree with the list's own `geo.continent`; the others are placed by their geography (Turkey, Cyprus and the
/// Caucasus in Europe, Kazakhstan in Asia, Mexico and Central America in North America, Egypt in Africa).
const TABLE: [(&str, Continent); 85] = [
    ("AE", AS),
    ("AL", EU),
    ("AM", EU),
    ("AR", SA),
    ("AT", EU),
    ("AU", OC),
    ("AZ", EU),
    ("BA", EU),
    ("BD", AS),
    ("BE", EU),
    ("BG", EU),
    ("BO", SA),
    ("BR", SA),
    ("BY", EU),
    ("CA", NA),
    ("CH", EU),
    ("CL", SA),
    ("CN", AS),
    ("CO", SA),
    ("CR", NA),
    ("CY", EU),
    ("CZ", EU),
    ("DE", EU),
    ("DK", EU),
    ("EC", SA),
    ("EE", EU),
    ("EG", AF),
    ("ES", EU),
    ("FI", EU),
    ("FR", EU),
    ("GB", EU),
    ("GE", EU),
    ("GR", EU),
    ("HK", AS),
    ("HR", EU),
    ("HU", EU),
    ("ID", AS),
    ("IE", EU),
    ("IL", AS),
    ("IN", AS),
    ("IS", EU),
    ("IT", EU),
    ("JP", AS),
    ("KE", AF),
    ("KH", AS),
    ("KR", AS),
    ("KZ", AS),
    ("LK", AS),
    ("LT", EU),
    ("LU", EU),
    ("LV", EU),
    ("MA", AF),
    ("MD", EU),
    ("ME", EU),
    ("MK", EU),
    ("MT", EU),
    ("MX", NA),
    ("MY", AS),
    ("NG", AF),
    ("NL", EU),
    ("NO", EU),
    ("NP", AS),
    ("NZ", OC),
    ("PA", NA),
    ("PE", SA),
    ("PH", AS),
    ("PK", AS),
    ("PL", EU),
    ("PT", EU),
    ("QA", AS),
    ("RO", EU),
    ("RS", EU),
    ("SA", AS),
    ("SE", EU),
    ("SG", AS),
    ("SI", EU),
    ("SK", EU),
    ("TH", AS),
    ("TR", EU),
    ("TW", AS),
    ("UA", EU),
    ("US", NA),
    ("UY", SA),
    ("VN", AS),
    ("ZA", AF),
];

impl Continent {
    /// Continent from the code of the server list (`geo.continent`); anything else is `None`.
    pub fn from_api(code: &str) -> Option<Self> {
        match code.trim().to_ascii_uppercase().as_str() {
            "EU" => Some(Self::Europe),
            "AS" => Some(Self::Asia),
            "NA" => Some(Self::NorthAmerica),
            "SA" => Some(Self::SouthAmerica),
            "AF" => Some(Self::Africa),
            "OC" => Some(Self::Oceania),
            _ => None,
        }
    }

    /// Continent of a known country (upper case code) according to Celata's table.
    pub fn of_code(country: &str) -> Option<Self> {
        TABLE
            .iter()
            .find(|(code, _)| *code == country)
            .map(|(_, continent)| *continent)
    }

    /// Name of the continent in `language`.
    pub fn name(self, language: Language) -> &'static str {
        let key = match self {
            Self::Europe => Key::ContinentEurope,
            Self::Asia => Key::ContinentAsia,
            Self::NorthAmerica => Key::ContinentNorthAmerica,
            Self::SouthAmerica => Key::ContinentSouthAmerica,
            Self::Africa => Key::ContinentAfrica,
            Self::Oceania => Key::ContinentOceania,
        };
        i18n::text(language, key)
    }
}

/// How the continent of a country that is not in the table was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// The server list names a continent.
    List,
    /// The continent of the nearest country with a known continent.
    Nearest,
    /// Nothing else is known: Europe.
    Fallback,
}

impl Step {
    /// Short word for the report of the maintainer test.
    pub fn word(self) -> &'static str {
        match self {
            Self::List => "from the list",
            Self::Nearest => "nearest country",
            Self::Fallback => "fallback",
        }
    }
}

/// Continent of a country according to the list: the value most of its servers carry; with a tie the one of
/// the first of these servers.
fn from_list(country: &Country) -> Option<Continent> {
    let values: Vec<Continent> = country.servers.iter().filter_map(|server| server.continent).collect();
    let mut best: Option<(Continent, usize)> = None;
    for value in &values {
        let count = values.iter().filter(|other| *other == value).count();
        if best.is_none_or(|(_, most)| count > most) {
            best = Some((*value, count));
        }
    }
    best.map(|(value, _)| value)
}

/// Positions of all servers of a country that have coordinates.
fn positions(country: &Country) -> impl Iterator<Item = (f64, f64)> + '_ {
    country
        .servers
        .iter()
        .filter_map(|server| server.position.map(Position::degrees))
}

/// Continent of the nearest country among `known`; the country listed first wins a tie.
fn nearest(country: &Country, countries: &[Country], known: &BTreeMap<String, Continent>) -> Option<Continent> {
    let own: Vec<(f64, f64)> = positions(country).collect();
    let mut best: Option<(f64, Continent)> = None;
    for other in countries {
        let Some(&continent) = known.get(&other.code) else {
            continue;
        };
        for point in positions(other) {
            for mine in &own {
                let distance = best::distance_km(*mine, point);
                if best.is_none_or(|(least, _)| distance < least) {
                    best = Some((distance, continent));
                }
            }
        }
    }
    best.map(|(_, continent)| continent)
}

/// Every country of the list with its continent and, for a country outside the table, the step that found it.
fn resolve(countries: &[Country]) -> BTreeMap<String, (Continent, Option<Step>)> {
    // Steps 1 and 2: the table, then the list.
    let mut known: BTreeMap<String, (Continent, Option<Step>)> = BTreeMap::new();
    for country in countries {
        if let Some(continent) = Continent::of_code(&country.code) {
            known.insert(country.code.clone(), (continent, None));
        } else if let Some(continent) = from_list(country) {
            known.insert(country.code.clone(), (continent, Some(Step::List)));
        }
    }
    let basis: BTreeMap<String, Continent> = known
        .iter()
        .map(|(code, (continent, _))| (code.clone(), *continent))
        .collect();
    // Steps 3 and 4.
    for country in countries {
        if known.contains_key(&country.code) {
            continue;
        }
        let entry = match nearest(country, countries, &basis) {
            Some(continent) => (continent, Some(Step::Nearest)),
            None => (Continent::Europe, Some(Step::Fallback)),
        };
        known.insert(country.code.clone(), entry);
    }
    known
}

/// The continent of every country of the list, found in four steps: Celata's table, the server list, the
/// nearest country, Europe. Exactly one entry per country.
pub fn assign(countries: &[Country]) -> BTreeMap<String, Continent> {
    resolve(countries)
        .into_iter()
        .map(|(code, (continent, _))| (code, continent))
        .collect()
}

/// A known country whose continent differs between Celata's table and the server list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deviation {
    pub code: String,
    /// English name from the list.
    pub name: String,
    pub own: Continent,
    pub list: Continent,
}

/// Countries for which the server list says something else than Celata's table. For the maintainer only.
pub fn deviations(countries: &[Country]) -> Vec<Deviation> {
    countries
        .iter()
        .filter_map(|country| {
            let own = Continent::of_code(&country.code)?;
            let list = from_list(country)?;
            (own != list).then(|| Deviation {
                code: country.code.clone(),
                name: country.original.clone(),
                own,
                list,
            })
        })
        .collect()
}

/// A country of the list that is not in Celata's table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unknown {
    pub code: String,
    /// English name from the list.
    pub name: String,
    pub assigned: Continent,
    pub step: Step,
}

/// Countries of the list that are not in Celata's table, with the continent they get. For the maintainer only.
pub fn unknown(countries: &[Country]) -> Vec<Unknown> {
    let resolved = resolve(countries);
    countries
        .iter()
        .filter(|country| Continent::of_code(&country.code).is_none())
        .filter_map(|country| {
            let (assigned, step) = *resolved.get(&country.code)?;
            Some(Unknown {
                code: country.code.clone(),
                name: country.original.clone(),
                assigned,
                step: step?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::names;
    use crate::servers::{Server, ServerLabel};

    fn server(continent: Option<Continent>, position: Option<(f64, f64)>) -> Server {
        Server {
            label: ServerLabel::Plain,
            original: "x".to_owned(),
            city: None,
            host: "x-v4.hideservers.net".to_owned(),
            free: false,
            automatic: false,
            derived: false,
            position: position.and_then(|(lat, lon)| Position::from_degrees(lat, lon)),
            no_best: false,
            continent,
        }
    }

    fn country(code: &str, servers: Vec<Server>) -> Country {
        Country {
            code: code.to_owned(),
            original: format!("Country {code}"),
            servers,
        }
    }

    #[test]
    fn the_table_has_every_known_code_exactly_once_and_nothing_else() {
        let table: Vec<&str> = TABLE.iter().map(|(code, _)| *code).collect();
        assert_eq!(table, names::CODES.to_vec());
        for code in names::CODES {
            assert!(Continent::of_code(code).is_some(), "{code}");
        }
        assert_eq!(Continent::of_code("XX"), None);
        assert_eq!(Continent::of_code("de"), None);
    }

    #[test]
    fn border_cases_and_assumptions_are_fixed() {
        for code in ["TR", "CY", "GE", "AM", "AZ"] {
            assert_eq!(Continent::of_code(code), Some(Continent::Europe), "{code}");
        }
        assert_eq!(Continent::of_code("KZ"), Some(Continent::Asia));
        for code in ["MX", "CR", "PA"] {
            assert_eq!(Continent::of_code(code), Some(Continent::NorthAmerica), "{code}");
        }
        assert_eq!(Continent::of_code("EG"), Some(Continent::Africa));
        for code in ["AE", "IL", "QA", "SA"] {
            assert_eq!(Continent::of_code(code), Some(Continent::Asia), "{code}");
        }
        assert_eq!(Continent::of_code("US"), Some(Continent::NorthAmerica));
        assert_eq!(Continent::of_code("BR"), Some(Continent::SouthAmerica));
        assert_eq!(Continent::of_code("NZ"), Some(Continent::Oceania));
    }

    #[test]
    fn codes_of_the_list_are_read_leniently() {
        let codes = [
            ("EU", Continent::Europe),
            ("AS", Continent::Asia),
            ("NA", Continent::NorthAmerica),
            ("SA", Continent::SouthAmerica),
            ("AF", Continent::Africa),
            ("OC", Continent::Oceania),
        ];
        for (code, continent) in codes {
            assert_eq!(Continent::from_api(code), Some(continent));
            assert_eq!(Continent::from_api(&code.to_lowercase()), Some(continent));
            assert_eq!(Continent::from_api(&format!(" {code} ")), Some(continent));
        }
        for other in ["AN", "", "Europe", "E U"] {
            assert_eq!(Continent::from_api(other), None, "{other:?}");
        }
    }

    #[test]
    fn names_exist_in_both_languages_and_the_order_is_fixed() {
        let en = ["Europe", "Asia", "North America", "South America", "Africa", "Oceania"];
        let de = ["Europa", "Asien", "Nordamerika", "Südamerika", "Afrika", "Ozeanien"];
        for (index, continent) in ALL.iter().enumerate() {
            assert_eq!(continent.name(Language::En), en[index]);
            assert_eq!(continent.name(Language::De), de[index]);
        }
        let mut sorted = ALL;
        sorted.sort();
        assert_eq!(sorted, ALL);
    }

    #[test]
    fn the_table_beats_the_list() {
        let germany = country("DE", vec![server(Some(Continent::Asia), None)]);
        let result = assign(std::slice::from_ref(&germany));
        assert_eq!(result["DE"], Continent::Europe);
        assert_eq!(
            deviations(std::slice::from_ref(&germany)),
            vec![Deviation {
                code: "DE".to_owned(),
                name: "Country DE".to_owned(),
                own: Continent::Europe,
                list: Continent::Asia,
            }]
        );
    }

    #[test]
    fn a_country_outside_the_table_follows_the_list_by_majority() {
        let xx = country(
            "XX",
            vec![
                server(Some(Continent::Africa), None),
                server(Some(Continent::Asia), None),
                server(Some(Continent::Asia), None),
                server(None, None),
            ],
        );
        assert_eq!(assign(std::slice::from_ref(&xx))["XX"], Continent::Asia);
        let found = unknown(std::slice::from_ref(&xx));
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].assigned, found[0].step), (Continent::Asia, Step::List));
    }

    #[test]
    fn a_tie_goes_to_the_first_server() {
        let xx = country(
            "XX",
            vec![
                server(Some(Continent::Oceania), None),
                server(Some(Continent::Africa), None),
            ],
        );
        assert_eq!(assign(&[xx])["XX"], Continent::Oceania);
    }

    #[test]
    fn a_built_name_has_no_continent_and_does_not_count() {
        let mut built = server(None, None);
        built.derived = true;
        let xx = country("XX", vec![built, server(Some(Continent::Africa), None)]);
        assert_eq!(assign(&[xx])["XX"], Continent::Africa);
    }

    #[test]
    fn an_unknown_country_without_a_value_takes_the_nearest_country() {
        let near_asia = country("XX", vec![server(None, Some((35.0, 139.0)))]);
        let japan = country("JP", vec![server(Some(Continent::Asia), Some((35.6, 139.7)))]);
        let germany = country("DE", vec![server(Some(Continent::Europe), Some((52.5, 13.4)))]);
        let result = assign(&[germany.clone(), near_asia.clone(), japan.clone()]);
        assert_eq!(result["XX"], Continent::Asia);
        let found = unknown(&[germany, near_asia, japan]);
        assert_eq!((found[0].code.as_str(), found[0].step), ("XX", Step::Nearest));
    }

    #[test]
    fn the_nearest_country_must_have_a_known_continent_itself() {
        // YY has no continent of its own and sits closer than JP, but only JP counts.
        let xx = country("XX", vec![server(None, Some((35.0, 139.0)))]);
        let yy = country("YY", vec![server(None, Some((35.1, 139.1)))]);
        let japan = country("JP", vec![server(Some(Continent::Asia), Some((10.0, 100.0)))]);
        let result = assign(&[xx, yy, japan]);
        assert_eq!(result["XX"], Continent::Asia);
        assert_eq!(result["YY"], Continent::Asia);
    }

    #[test]
    fn a_tie_in_distance_goes_to_the_country_listed_first() {
        let xx = country("XX", vec![server(None, Some((0.0, 0.0)))]);
        let first = country("AA", vec![server(Some(Continent::Africa), Some((0.0, 10.0)))]);
        let second = country("BB", vec![server(Some(Continent::Oceania), Some((0.0, -10.0)))]);
        assert_eq!(
            assign(&[xx.clone(), first.clone(), second.clone()])["XX"],
            Continent::Africa
        );
        assert_eq!(assign(&[xx, second, first])["XX"], Continent::Oceania);
    }

    #[test]
    fn a_country_without_coordinates_falls_back_to_europe() {
        let xx = country("XX", vec![server(None, None)]);
        let japan = country("JP", vec![server(Some(Continent::Asia), Some((35.6, 139.7)))]);
        let result = assign(&[xx.clone(), japan.clone()]);
        assert_eq!(result["XX"], Continent::Europe);
        let found = unknown(&[xx, japan]);
        assert_eq!((found[0].assigned, found[0].step), (Continent::Europe, Step::Fallback));
    }

    #[test]
    fn without_any_other_coordinates_the_fallback_applies() {
        let xx = country("XX", vec![server(None, Some((35.0, 139.0)))]);
        assert_eq!(assign(&[xx])["XX"], Continent::Europe);
    }

    #[test]
    fn every_country_gets_exactly_one_entry() {
        let list = vec![
            country("DE", vec![server(None, None)]),
            country("XX", vec![server(Some(Continent::Africa), None)]),
            country("YY", vec![server(None, None)]),
        ];
        let result = assign(&list);
        assert_eq!(result.len(), 3);
        for entry in &list {
            assert!(result.contains_key(&entry.code));
        }
        assert!(deviations(&list).is_empty());
        assert_eq!(unknown(&list).len(), 2);
    }

    #[test]
    fn the_real_list_agrees_with_the_table_and_knows_every_country() {
        let list = crate::servers::parse_list(include_bytes!("../tests/fixtures/servers-2026-11-19.json")).unwrap();
        assert_eq!(list.len(), 57);
        let result = assign(&list);
        assert_eq!(result.len(), 57);
        assert!(deviations(&list).is_empty());
        assert!(unknown(&list).is_empty());
        let count = |continent: Continent| result.values().filter(|value| **value == continent).count();
        let counts: Vec<usize> = ALL.iter().map(|continent| count(*continent)).collect();
        assert_eq!(counts, [31, 13, 3, 5, 3, 2]);
    }

    #[test]
    fn the_report_words_are_short() {
        assert_eq!(Step::List.word(), "from the list");
        assert_eq!(Step::Nearest.word(), "nearest country");
        assert_eq!(Step::Fallback.word(), "fallback");
    }
}
