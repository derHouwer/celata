// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! Known countries of the server list and sorting of names.
//!
//! The names themselves are in `src/i18n`, per language. The API delivers English names; unknown
//! countries and places appear with that name, so that new servers show up anyway.

/// Country codes (ISO 3166-1, upper case) of the known countries. The only list, also for the flags in
/// `src/flags.rs`; every language in `src/i18n` has a name for every code (test there).
pub const CODES: [&str; 85] = [
    "AE", "AL", "AM", "AR", "AT", "AU", "AZ", "BA", "BD", "BE", "BG", "BO", "BR", "BY", "CA", "CH", "CL", "CN", "CO",
    "CR", "CY", "CZ", "DE", "DK", "EC", "EE", "EG", "ES", "FI", "FR", "GB", "GE", "GR", "HK", "HR", "HU", "ID", "IE",
    "IL", "IN", "IS", "IT", "JP", "KE", "KH", "KR", "KZ", "LK", "LT", "LU", "LV", "MA", "MD", "ME", "MK", "MT", "MX",
    "MY", "NG", "NL", "NO", "NP", "NZ", "PA", "PE", "PH", "PK", "PL", "PT", "QA", "RO", "RS", "SA", "SE", "SG", "SI",
    "SK", "TH", "TR", "TW", "UA", "US", "UY", "VN", "ZA",
];

/// Is the country code known?
pub fn is_known(code: &str) -> bool {
    CODES.contains(&code)
}

/// Key for sorting: umlauts like their base letters, so that the German name of Egypt is found under A.
pub fn sort_key(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| match c {
            'ä' => 'a',
            'ö' => 'o',
            'ü' => 'u',
            other => other,
        })
        .collect::<String>()
        .replace('ß', "ss")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn umlaute_werden_beim_sortieren_wie_grundbuchstaben_behandelt() {
        let mut names = vec!["Österreich", "Ägypten", "Albanien", "Niederlande", "Belgien"];
        names.sort_by_key(|name| sort_key(name));
        assert_eq!(
            names,
            vec!["Ägypten", "Albanien", "Belgien", "Niederlande", "Österreich"]
        );
    }
}
