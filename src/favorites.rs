// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! Favourite locations.
//!
//! A favourite is only a host name, stored in the own settings file (`app.favorites`, newest first). Tab,
//! name, country and flag follow when showing from the server list and the streaming table. Favourites
//! never go to the service or into the network.

use crate::best;
use crate::servers::{self, Country};
use crate::streaming::{self, StreamingServer};

/// Tab of the page "Locations" a favourite belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Standard,
    Streaming,
}

/// Whether a favourite can be selected right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// The location is in the list and can be selected.
    Available,
    /// Not in the list (removed by hide.me, or a built country name that failed): shown with its host name
    /// as "not available", only the star still works.
    Missing,
    /// The server list is still loading.
    Loading,
}

impl State {
    /// Value for the user interface.
    pub fn as_str(self) -> &'static str {
        match self {
            State::Available => "available",
            State::Missing => "missing",
            State::Loading => "loading",
        }
    }
}

/// Host name in the stored form: without white space, in lower case. `None` if it is not a permitted
/// host name. The mark [`best::BEST`] ("Best location") is permitted as well.
pub fn normalize(host: &str) -> Option<String> {
    let host = host.trim().to_ascii_lowercase();
    (servers::is_valid_host(&host) || host == best::BEST).then_some(host)
}

/// Switches `host`: a favourite is removed, any other location becomes the newest favourite at the
/// front. Returns whether `host` is a favourite now. An invalid host name changes nothing.
pub fn toggle(favorites: &mut Vec<String>, host: &str) -> bool {
    let Some(host) = normalize(host) else {
        return false;
    };
    if let Some(index) = favorites.iter().position(|known| *known == host) {
        favorites.remove(index);
        false
    } else {
        favorites.insert(0, host);
        true
    }
}

/// Tab of a host: "Streaming" for the hosts of the streaming table, otherwise "Standard".
pub fn tab_of(host: &str) -> Tab {
    if streaming::all().iter().any(|server| server.host == host) {
        Tab::Streaming
    } else {
        Tab::Standard
    }
}

/// State of a favourite. `countries` is the loaded server list (empty while loading or after a failure),
/// `list_loading` tells whether it is still loading, `streaming_visible` holds the streaming servers that
/// are shown.
///
/// "Best location" is never missing: it is loading while the list loads without countries, otherwise available;
/// connecting then tells if no list is there.
pub fn resolve(host: &str, countries: &[Country], list_loading: bool, streaming_visible: &[StreamingServer]) -> State {
    if host == best::BEST {
        return if list_loading && countries.is_empty() {
            State::Loading
        } else {
            State::Available
        };
    }
    match tab_of(host) {
        Tab::Streaming => {
            if streaming_visible.iter().any(|server| server.host == host) {
                State::Available
            } else {
                State::Missing
            }
        }
        Tab::Standard => {
            if countries
                .iter()
                .flat_map(|country| &country.servers)
                .any(|server| server.host == host)
            {
                State::Available
            } else if list_loading && countries.is_empty() {
                State::Loading
            } else {
                State::Missing
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::servers::parse_list;
    use crate::streaming::Check;
    use std::collections::BTreeSet;

    const LIST: &[u8] = include_bytes!("../tests/fixtures/servers-2026-11-19.json");

    #[test]
    fn neuer_favorit_steht_vorne() {
        let mut favorites = vec!["a.hideservers.net".to_owned()];
        assert!(toggle(&mut favorites, "b.hideservers.net"));
        assert_eq!(favorites, ["b.hideservers.net", "a.hideservers.net"]);
    }

    #[test]
    fn vorhandener_favorit_wird_entfernt_und_zweimal_ist_wie_vorher() {
        let before = vec!["a.hideservers.net".to_owned(), "b.hideservers.net".to_owned()];
        let mut favorites = before.clone();
        assert!(!toggle(&mut favorites, "a.hideservers.net"));
        assert_eq!(favorites, ["b.hideservers.net"]);
        let mut twice = before.clone();
        toggle(&mut twice, "c.hideservers.net");
        toggle(&mut twice, "c.hideservers.net");
        assert_eq!(twice, before);
    }

    #[test]
    fn erneut_markiert_steht_vorne() {
        let mut favorites = vec!["a.hideservers.net".to_owned(), "b.hideservers.net".to_owned()];
        toggle(&mut favorites, "b.hideservers.net");
        toggle(&mut favorites, "b.hideservers.net");
        assert_eq!(favorites, ["b.hideservers.net", "a.hideservers.net"]);
    }

    #[test]
    fn schreibweise_wird_vereinheitlicht_und_ungueltiges_aendert_nichts() {
        let mut favorites = vec!["a.hideservers.net".to_owned()];
        assert!(!toggle(&mut favorites, "  A.HideServers.NET "));
        assert!(favorites.is_empty());
        for bad in ["", "example.org", "a.b.hideservers.net", "a\".hideservers.net"] {
            assert!(!toggle(&mut favorites, bad), "{bad}");
            assert!(favorites.is_empty(), "{bad}");
        }
    }

    #[test]
    fn reiter_nach_dem_host() {
        for server in streaming::all() {
            assert_eq!(tab_of(&server.host), Tab::Streaming, "{}", server.host);
        }
        assert_eq!(tab_of("de.hideservers.net"), Tab::Standard);
        assert_eq!(tab_of("unbekannt.hideservers.net"), Tab::Standard);
    }

    #[test]
    fn zustand_eines_favoriten() {
        let countries = parse_list(LIST).unwrap();
        let visible = streaming::all();
        let japan = "streaming-jp.hideservers.net";
        assert_eq!(
            resolve("de.hideservers.net", &countries, false, &visible),
            State::Available
        );
        assert_eq!(
            resolve("fi.hideservers.net", &countries, false, &visible),
            State::Available
        );
        assert_eq!(resolve("de.hideservers.net", &[], true, &visible), State::Loading);
        assert_eq!(resolve("de.hideservers.net", &[], false, &visible), State::Missing);
        assert_eq!(
            resolve("unbekannt.hideservers.net", &countries, false, &visible),
            State::Missing
        );
        // Loading again with a list already shown: an unknown host stays missing.
        assert_eq!(
            resolve("unbekannt.hideservers.net", &countries, true, &visible),
            State::Missing
        );
        // Built name that failed.
        let mut dropped = countries.clone();
        servers::drop_derived(&mut dropped, &BTreeSet::from(["fi.hideservers.net".to_owned()]));
        assert_eq!(resolve("fi.hideservers.net", &dropped, false, &visible), State::Missing);
        // Streaming: available while visible, independent of the server list.
        assert_eq!(resolve(japan, &[], true, &visible), State::Available);
        let hidden = streaming::visible(&Check::Checked(BTreeSet::from([japan.to_owned()])));
        assert_eq!(resolve(japan, &countries, false, &hidden), State::Missing);
    }

    // --- "Best location" (feature 013) ---

    #[test]
    fn bester_standort_als_favorit() {
        assert_eq!(normalize("best").as_deref(), Some("best"));
        assert_eq!(normalize(" BEST ").as_deref(), Some("best"));
        assert_eq!(normalize("bester"), None);
        let mut favorites = vec!["frankfurt1.hideservers.net".to_owned()];
        assert!(toggle(&mut favorites, "best"));
        assert_eq!(favorites, ["best", "frankfurt1.hideservers.net"]);
        assert!(!toggle(&mut favorites, "best"));
        assert_eq!(favorites, ["frankfurt1.hideservers.net"]);
        assert_eq!(tab_of("best"), Tab::Standard);
    }

    #[test]
    fn bester_standort_ist_nie_nicht_verfuegbar() {
        let countries = parse_list(LIST).unwrap();
        let visible = streaming::all();
        assert_eq!(resolve("best", &[], true, &visible), State::Loading);
        assert_eq!(resolve("best", &countries, true, &visible), State::Available);
        assert_eq!(resolve("best", &countries, false, &visible), State::Available);
        // After a failed loading the list is empty: still available, connecting then tells.
        assert_eq!(resolve("best", &[], false, &visible), State::Available);
    }

    #[test]
    fn zustand_als_text() {
        assert_eq!(State::Available.as_str(), "available");
        assert_eq!(State::Missing.as_str(), "missing");
        assert_eq!(State::Loading.as_str(), "loading");
    }
}
