// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! Location of the user for "Best location", taken only from this computer.
//!
//! First choice is the place the time zone database of the system assigns to the configured time zone
//! (`zone.tab`, `zone1970.tab`, links from `tzdata.zi`). If the time zone has no place, the country of the
//! system language counts instead. Nothing here goes into the network, starts a program or writes a file;
//! nothing is kept after the call. Only [`system_origin`] looks at the system, and only `main.rs` calls it
//! when connecting through "Best location".

use std::env;
use std::path::Path;

use crate::best::Origin;
use crate::files;

/// Directory of the time zone database on Debian, Ubuntu, Fedora and Arch.
const ZONEINFO: &str = "/usr/share/zoneinfo";
/// Link to the configured time zone.
const LOCALTIME: &str = "/etc/localtime";
/// Name of the configured time zone (Debian, Ubuntu).
const ETC_TIMEZONE: &str = "/etc/timezone";
/// Largest file that is read; today the largest one (`tzdata.zi`) has about 105 KiB.
const MAX_FILE: u64 = 512 * 1024;
/// Longest accepted name of a time zone.
const MAX_NAME: usize = 64;
/// Part of a path in front of the name of a time zone.
const ZONEINFO_PART: &str = "zoneinfo/";

/// Checks a name of a time zone: 1 to 64 characters of `A–Z a–z 0–9 / _ - +`, no `..`, not starting with `/`.
/// A value without `/` that contains a digit is a POSIX rule such as `CET-1CEST`, not a name.
fn valid_name(name: &str) -> Option<String> {
    let allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '-' | '+');
    let posix_rule = !name.contains('/') && name.chars().any(|c| c.is_ascii_digit());
    let valid = !name.is_empty()
        && name.chars().count() <= MAX_NAME
        && name.chars().all(allowed)
        && !name.contains("..")
        && !name.starts_with('/')
        && !posix_rule;
    valid.then(|| name.to_owned())
}

/// Part of a path behind the last `zoneinfo/`, without a leading `posix/` or `right/`.
fn after_zoneinfo(path: &str) -> Option<&str> {
    let index = path.rfind(ZONEINFO_PART)?;
    let name = &path[index + ZONEINFO_PART.len()..];
    Some(
        name.strip_prefix("posix/")
            .or_else(|| name.strip_prefix("right/"))
            .unwrap_or(name),
    )
}

/// Name of the configured time zone from the first usable source: the variable `TZ`, the target of the link
/// `/etc/localtime`, the first line of `/etc/timezone`.
pub fn zone_name(tz: Option<&str>, localtime_link: Option<&Path>, etc_timezone: Option<&str>) -> Option<String> {
    let from_tz = tz.and_then(|tz| {
        let tz = tz.strip_prefix(':').unwrap_or(tz);
        if tz.starts_with('/') {
            valid_name(after_zoneinfo(tz)?)
        } else {
            valid_name(tz)
        }
    });
    from_tz
        .or_else(|| valid_name(after_zoneinfo(localtime_link?.to_str()?)?))
        .or_else(|| valid_name(etc_timezone?.lines().next()?.trim()))
}

/// Splits `digits` into degrees, minutes and seconds; minutes and seconds must be below 60.
fn angle(digits: &str, degree_digits: usize, max_degrees: u32) -> Option<f64> {
    if !digits.bytes().all(|byte| byte.is_ascii_digit())
        || (digits.len() != degree_digits + 2 && digits.len() != degree_digits + 4)
    {
        return None;
    }
    let number = |range: std::ops::Range<usize>| digits.get(range)?.parse::<u32>().ok();
    let degrees = number(0..degree_digits)?;
    let minutes = number(degree_digits..degree_digits + 2)?;
    let seconds = if digits.len() == degree_digits + 4 {
        number(degree_digits + 2..degree_digits + 4)?
    } else {
        0
    };
    if minutes >= 60 || seconds >= 60 || degrees > max_degrees {
        return None;
    }
    Some(f64::from(degrees) + f64::from(minutes) / 60.0 + f64::from(seconds) / 3600.0)
}

/// Coordinates in the form of the tables (ISO 6709): `±DDMM±DDDMM` or `±DDMMSS±DDDMMSS`, in degrees.
pub fn parse_coordinates(text: &str) -> Option<(f64, f64)> {
    let sign = |c: char| match c {
        '+' => Some(1.0),
        '-' => Some(-1.0),
        _ => None,
    };
    let lat_sign = sign(text.chars().next()?)?;
    let rest = &text[1..];
    let split = rest.find(['+', '-'])?;
    let lon_sign = sign(rest[split..].chars().next()?)?;
    let lat = angle(&rest[..split], 2, 90)?;
    let lon = angle(&rest[split + 1..], 3, 180)?;
    Some((lat_sign * lat, lon_sign * lon))
}

/// Coordinates of `name` in the first table that lists it (column 3, coordinates in column 2).
fn find_in(tables: &[&str], name: &str) -> Option<(f64, f64)> {
    tables.iter().flat_map(|table| table.lines()).find_map(|line| {
        if line.starts_with('#') {
            return None;
        }
        let columns: Vec<&str> = line.split('\t').collect();
        match columns.as_slice() {
            [_, coordinates, zone, ..] if *zone == name => parse_coordinates(coordinates),
            _ => None,
        }
    })
}

/// Coordinates of the time zone `name`: from the tables in order (`zone.tab`, then `zone1970.tab`); otherwise
/// one step along a link `L <target> <name>` from `tzdata.zi` and the same search for the target.
pub fn lookup(tables: &[&str], links: &str, name: &str) -> Option<(f64, f64)> {
    find_in(tables, name).or_else(|| {
        let target = links.lines().find_map(|line| {
            let mut parts = line.split_whitespace();
            match (parts.next(), parts.next(), parts.next(), parts.next()) {
                (Some("L"), Some(target), Some(link), None) if link == name => Some(target),
                _ => None,
            }
        })?;
        find_in(tables, target)
    })
}

/// Country code from the system language: the first non-empty variable of `LC_ALL`, `LC_MESSAGES`, `LANG`,
/// part after `_` up to `.` or `@`, exactly two ASCII letters, in upper case (`de_DE.utf8` → `DE`).
pub fn locale_country(lc_all: Option<&str>, lc_messages: Option<&str>, lang: Option<&str>) -> Option<String> {
    let value = [lc_all, lc_messages, lang]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|value| !value.is_empty())?;
    let (_, after) = value.split_once('_')?;
    let code = after.split(['.', '@']).next()?;
    (code.len() == 2 && code.bytes().all(|byte| byte.is_ascii_alphabetic())).then(|| code.to_ascii_uppercase())
}

/// Location from the coordinates of the time zone, otherwise from the country of the system language.
pub fn origin_from(zone: Option<(f64, f64)>, country: Option<String>) -> Option<Origin> {
    match (zone, country) {
        (Some((lat, lon)), _) => Some(Origin::Point { lat, lon }),
        (None, Some(country)) => Some(Origin::Country(country)),
        (None, None) => None,
    }
}

/// Reads a small text file; missing, too large or unreadable counts as absent.
fn read_text(path: &Path) -> Option<String> {
    let content = files::read_limited(path, MAX_FILE).ok().flatten()?;
    String::from_utf8(content).ok()
}

/// Location of the user from this computer. Reads only the variables `TZ`, `LC_ALL`, `LC_MESSAGES`, `LANG`,
/// the link `/etc/localtime`, `/etc/timezone` and the three tables in `/usr/share/zoneinfo`. Nothing is kept,
/// so that a changed time zone applies at once.
pub fn system_origin() -> Option<Origin> {
    let tz = env::var("TZ").ok();
    let link = std::fs::read_link(LOCALTIME).ok();
    let etc_timezone = read_text(Path::new(ETC_TIMEZONE));
    let point = zone_name(tz.as_deref(), link.as_deref(), etc_timezone.as_deref()).and_then(|name| {
        let dir = Path::new(ZONEINFO);
        let zone_tab = read_text(&dir.join("zone.tab")).unwrap_or_default();
        let zone1970_tab = read_text(&dir.join("zone1970.tab")).unwrap_or_default();
        let links = read_text(&dir.join("tzdata.zi")).unwrap_or_default();
        lookup(&[&zone_tab, &zone1970_tab], &links, &name)
    });
    let country = if point.is_none() {
        let variable = |name: &str| env::var(name).ok();
        locale_country(
            variable("LC_ALL").as_deref(),
            variable("LC_MESSAGES").as_deref(),
            variable("LANG").as_deref(),
        )
    } else {
        None
    };
    origin_from(point, country)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ZONE_TAB: &str = include_str!("../tests/fixtures/tz/zone.tab");
    const ZONE1970_TAB: &str = include_str!("../tests/fixtures/tz/zone1970.tab");
    const LINKS: &str = include_str!("../tests/fixtures/tz/links.zi");

    fn coordinates(name: &str) -> Option<(f64, f64)> {
        lookup(&[ZONE_TAB, ZONE1970_TAB], LINKS, name)
    }

    fn close(actual: (f64, f64), expected: (f64, f64)) -> bool {
        (actual.0 - expected.0).abs() < 0.001 && (actual.1 - expected.1).abs() < 0.001
    }

    #[test]
    fn koordinaten_bekannter_zeitzonen() {
        let berlin = coordinates("Europe/Berlin").unwrap();
        assert!(close(berlin, (52.5, 13.367)), "{berlin:?}");
        // Only in zone.tab; zone1970.tab merges it into Europe/Berlin.
        let copenhagen = coordinates("Europe/Copenhagen").unwrap();
        assert!(close(copenhagen, (55.667, 12.583)), "{copenhagen:?}");
        assert!(!ZONE1970_TAB.contains("\tEurope/Copenhagen"));
        let new_york = coordinates("America/New_York").unwrap();
        assert!(close(new_york, (40.714, -74.006)), "{new_york:?}");
    }

    #[test]
    fn verweis_wird_einen_schritt_verfolgt() {
        assert_eq!(coordinates("US/Central"), coordinates("America/Chicago"));
        assert!(coordinates("US/Central").is_some());
        // Without the link file the old name is unknown.
        assert_eq!(lookup(&[ZONE_TAB, ZONE1970_TAB], "", "US/Central"), None);
    }

    #[test]
    fn zeitzonen_ohne_ort() {
        for name in ["UTC", "Etc/UTC", "Etc/GMT+1", "Atlantis/Nowhere", ""] {
            assert_eq!(coordinates(name), None, "{name}");
        }
    }

    #[test]
    fn tabellen_kommentare_und_kurze_zeilen() {
        let table = "# DE\t+5230+01322\tEurope/Berlin\nDE\t+5230+01322\nXX\tkaputt\tEurope/Broken\nXX\t+1000+01000\tEurope/Broken\n";
        assert_eq!(lookup(&[table], "", "DE"), None);
        // A broken line does not stop the search.
        let broken = lookup(&[table], "", "Europe/Broken").unwrap();
        assert!(close(broken, (10.0, 10.0)), "{broken:?}");
    }

    #[test]
    fn namen_aus_tz() {
        let link = Path::new("../usr/share/zoneinfo/Europe/Paris");
        assert_eq!(
            zone_name(Some(":Europe/Berlin"), Some(link), None).as_deref(),
            Some("Europe/Berlin")
        );
        assert_eq!(
            zone_name(Some("/usr/share/zoneinfo/Europe/Berlin"), Some(link), None).as_deref(),
            Some("Europe/Berlin")
        );
        assert_eq!(
            zone_name(Some("Europe/Berlin"), None, None).as_deref(),
            Some("Europe/Berlin")
        );
        // "UTC" is a name (without a place); the link does not count then.
        assert_eq!(zone_name(Some("UTC"), Some(link), None).as_deref(), Some("UTC"));
        // A POSIX rule is not a name: on with the link.
        assert_eq!(
            zone_name(Some("CET-1CEST"), Some(link), None).as_deref(),
            Some("Europe/Paris")
        );
        assert_eq!(
            zone_name(Some("CET-1CEST,M3.5.0,M10.5.0/3"), Some(link), None).as_deref(),
            Some("Europe/Paris")
        );
        // An absolute path without "zoneinfo/" is not a name.
        assert_eq!(
            zone_name(Some("/etc/irgendwas"), Some(link), None).as_deref(),
            Some("Europe/Paris")
        );
        assert_eq!(zone_name(Some(""), Some(link), None).as_deref(), Some("Europe/Paris"));
    }

    #[test]
    fn namen_aus_dem_verweis_und_etc_timezone() {
        assert_eq!(
            zone_name(None, Some(Path::new("../usr/share/zoneinfo/posix/Europe/Berlin")), None).as_deref(),
            Some("Europe/Berlin")
        );
        assert_eq!(
            zone_name(
                None,
                Some(Path::new("/usr/share/zoneinfo/right/America/New_York")),
                None
            )
            .as_deref(),
            Some("America/New_York")
        );
        assert_eq!(
            zone_name(None, Some(Path::new("/usr/share/zoneinfo/Europe/Berlin")), None).as_deref(),
            Some("Europe/Berlin")
        );
        // Without "zoneinfo/" in the link: on with /etc/timezone.
        assert_eq!(
            zone_name(
                None,
                Some(Path::new("/etc/elsewhere")),
                Some("  Europe/Vienna  \nzweite Zeile")
            )
            .as_deref(),
            Some("Europe/Vienna")
        );
        assert_eq!(
            zone_name(None, None, Some("Europe/Vienna\n")).as_deref(),
            Some("Europe/Vienna")
        );
        assert_eq!(zone_name(None, None, None), None);
    }

    #[test]
    fn ungueltige_namen() {
        let long = format!("Europe/{}", "a".repeat(58));
        assert_eq!(long.chars().count(), 65);
        let just_fits = format!("Europe/{}", "a".repeat(57));
        for bad in [
            "Europe/../etc/passwd",
            "../Europe/Berlin",
            long.as_str(),
            "Europe/Ber lin",
            "Europe/Berlin;",
            "/Europe/Berlin",
        ] {
            assert_eq!(zone_name(Some(bad), None, None), None, "{bad}");
            assert_eq!(zone_name(None, None, Some(bad)), None, "{bad}");
        }
        assert_eq!(
            zone_name(Some(&just_fits), None, None).as_deref(),
            Some(just_fits.as_str())
        );
        assert_eq!(
            zone_name(None, Some(Path::new("/usr/share/zoneinfo/../../etc/passwd")), None),
            None
        );
    }

    #[test]
    fn koordinatenformat() {
        let berlin = parse_coordinates("+5230+01322").unwrap();
        assert!(close(berlin, (52.5, 13.3667)), "{berlin:?}");
        let sydney = parse_coordinates("-3352+15113").unwrap();
        assert!(close(sydney, (-33.8667, 151.2167)), "{sydney:?}");
        let new_york = parse_coordinates("+404251-0740023").unwrap();
        assert!(close(new_york, (40.7142, -74.0064)), "{new_york:?}");
        for bad in [
            "+5260+01322",
            "+5230+01360",
            "+523060+01322",
            "5230+01322",
            "+5230",
            "+5230+0132",
            "",
            "+",
            "+52a0+01322",
            "+9100+00000",
            "+0000+18100",
        ] {
            assert_eq!(parse_coordinates(bad), None, "{bad}");
        }
    }

    #[test]
    fn land_aus_der_systemsprache() {
        assert_eq!(locale_country(None, None, Some("de_DE.utf8")).as_deref(), Some("DE"));
        assert_eq!(
            locale_country(None, None, Some("en_GB.UTF-8@euro")).as_deref(),
            Some("GB")
        );
        assert_eq!(locale_country(None, None, Some("pt_BR")).as_deref(), Some("BR"));
        assert_eq!(locale_country(None, None, Some("de_at.UTF-8")).as_deref(), Some("AT"));
        for value in ["C.UTF-8", "POSIX", "C", "de", "de_DEU.UTF-8", "de_1E", ""] {
            assert_eq!(locale_country(None, None, Some(value)), None, "{value}");
        }
        assert_eq!(locale_country(None, None, None), None);
    }

    #[test]
    fn reihenfolge_der_sprachvariablen() {
        assert_eq!(
            locale_country(Some("fr_FR.UTF-8"), Some("it_IT.UTF-8"), Some("de_DE.UTF-8")).as_deref(),
            Some("FR")
        );
        assert_eq!(
            locale_country(None, Some("it_IT.UTF-8"), Some("de_DE.UTF-8")).as_deref(),
            Some("IT")
        );
        // Empty values are skipped.
        assert_eq!(
            locale_country(Some(""), Some("  "), Some("de_DE.UTF-8")).as_deref(),
            Some("DE")
        );
        // The first non-empty variable decides, even without a country.
        assert_eq!(locale_country(Some("C.UTF-8"), None, Some("de_DE.UTF-8")), None);
    }

    #[test]
    fn ort_aus_zeitzone_oder_land() {
        assert_eq!(
            origin_from(Some((52.5, 13.4)), Some("FR".to_owned())),
            Some(Origin::Point { lat: 52.5, lon: 13.4 })
        );
        assert_eq!(
            origin_from(None, Some("FR".to_owned())),
            Some(Origin::Country("FR".to_owned()))
        );
        assert_eq!(origin_from(None, None), None);
    }
}
