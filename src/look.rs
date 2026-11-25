// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! Derives from state and lists what the user interface shows.
//!
//! The user interface (`ui/*.slint`) decides nothing that depends on the state. It receives the results
//! of these functions as finished values.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::best;
use crate::continent::{self, Continent};
use crate::daemon::ServiceState;
use crate::favorites;
use crate::i18n::{self, Key, Language, Phrase};
use crate::names;
use crate::servers::{self, Country};
use crate::state::{DisplayState, StatusView};
use crate::streaming::{self, StreamingServer};

/// Colour of the status dot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Ok,
    Busy,
    Alert,
    Idle,
}

impl Tone {
    /// Value for the user interface.
    pub fn as_str(self) -> &'static str {
        match self {
            Tone::Ok => "ok",
            Tone::Busy => "busy",
            Tone::Alert => "alert",
            Tone::Idle => "idle",
        }
    }
}

/// Colour of the status dot for the display state. `None` means "Checking status …".
pub fn tone(display: Option<&DisplayState>) -> Tone {
    let Some(display) = display else { return Tone::Idle };
    // Deliberately without `_`: a new state is meant to cause a compile error here.
    match display {
        DisplayState::NotConnected => Tone::Idle,
        DisplayState::Connecting
        | DisplayState::Disconnecting
        | DisplayState::Switching { .. }
        | DisplayState::Reapplying { .. }
        | DisplayState::ServiceStarting { blocked: false } => Tone::Busy,
        DisplayState::Connected { .. } => Tone::Ok,
        DisplayState::ConnectingBlocked
        | DisplayState::Blocked
        | DisplayState::ServiceUnreachableBlocked
        | DisplayState::ServiceUnreachable
        | DisplayState::NoPermission
        | DisplayState::ServiceStarting { blocked: true } => Tone::Alert,
    }
}

/// Colour of the connection state for the main button, the system tray icon and the window icon.
///
/// Deliberately more reserved than [`tone`] (the status dot): green only while a connection exists, red only
/// while the internet block exists. Every transition stays blue, so the colour never promises more
/// protection than there is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateColor {
    /// Blue, as before.
    Normal,
    /// Green: the connection exists.
    Connected,
    /// Red: the internet is blocked and no VPN is connected.
    Blocked,
}

impl StateColor {
    /// Value for the user interface.
    pub fn as_str(self) -> &'static str {
        match self {
            StateColor::Normal => "normal",
            StateColor::Connected => "connected",
            StateColor::Blocked => "blocked",
        }
    }
}

/// Colour of the connection state for the display state. `None` means "Checking status …".
pub fn state_color(display: Option<&DisplayState>) -> StateColor {
    let Some(display) = display else {
        return StateColor::Normal;
    };
    // Deliberately without `_`: a new state is meant to cause a compile error here.
    match display {
        DisplayState::Connected { .. } => StateColor::Connected,
        // The block exists, also while connecting out of it and while the service is unreachable.
        DisplayState::Blocked
        | DisplayState::ConnectingBlocked
        | DisplayState::ServiceUnreachableBlocked
        | DisplayState::ServiceStarting { blocked: true } => StateColor::Blocked,
        DisplayState::NotConnected
        | DisplayState::Connecting
        | DisplayState::Disconnecting
        | DisplayState::Switching { .. }
        | DisplayState::Reapplying { .. }
        | DisplayState::ServiceUnreachable
        | DisplayState::NoPermission
        | DisplayState::ServiceStarting { blocked: false } => StateColor::Normal,
    }
}

/// What a click on the main button triggers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainAction {
    Connect,
    Disconnect,
    /// Not clickable.
    None,
}

impl MainAction {
    /// Value for the user interface.
    pub fn as_str(self) -> &'static str {
        match self {
            MainAction::Connect => "connect",
            MainAction::Disconnect => "disconnect",
            MainAction::None => "none",
        }
    }
}

/// What the right, red area of the main button offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SideAction {
    /// "Block internet", without connecting.
    Route,
    /// "Unblock internet".
    Unblock,
    /// No right area.
    None,
}

impl SideAction {
    /// Value for the user interface.
    pub fn as_str(self) -> &'static str {
        match self {
            SideAction::Route => "route",
            SideAction::Unblock => "unblock",
            SideAction::None => "none",
        }
    }

    /// Text of the whole button in the red state: from the moment the mouse enters the right area until
    /// it leaves the button, and for as long as the keyboard focus is on the right area. `None` exactly
    /// when the right area offers nothing.
    pub fn label(self) -> Option<Key> {
        match self {
            SideAction::Route => Some(Key::ButtonBlock),
            SideAction::Unblock => Some(Key::ButtonUnblock),
            SideAction::None => None,
        }
    }
}

/// Text and effect of the main button of the overview: main part and right area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MainButton {
    pub label: Key,
    pub action: MainAction,
    pub side: SideAction,
    /// `None` exactly when `side` offers nothing.
    pub side_label: Option<Key>,
}

/// Main button. The order of the checks matters: during a switch `can_disconnect` is true, yet the main
/// button stays disabled; "Disconnect" then exists as a small button of its own.
///
/// The right area offers "Unblock internet" as soon as that is possible (`can_unblock`, also during an
/// own connection attempt from the blocked state), otherwise "Block internet" in "Not connected" with
/// no operation running. Whether the kill switch is on in the settings is combined by the user interface
/// (`side-available`); `Monitor::begin_route` refuses otherwise.
pub fn main_button(display: Option<&DisplayState>, view: &StatusView) -> MainButton {
    let button = |label, action, side: SideAction| MainButton {
        label,
        action,
        side,
        side_label: side.label(),
    };
    let unblock = if view.can_unblock {
        SideAction::Unblock
    } else {
        SideAction::None
    };
    if matches!(display, Some(DisplayState::Reapplying { .. })) {
        button(Key::ButtonReconnecting, MainAction::None, SideAction::None)
    } else if view.switching {
        button(Key::ButtonSwitching, MainAction::None, SideAction::None)
    } else if view.connecting {
        button(Key::ButtonConnecting, MainAction::None, unblock)
    } else if display == Some(&DisplayState::Disconnecting) {
        button(Key::ButtonDisconnecting, MainAction::None, SideAction::None)
    } else if view.routing {
        button(Key::ButtonBlocking, MainAction::None, SideAction::None)
    } else if view.can_disconnect {
        button(Key::ButtonDisconnect, MainAction::Disconnect, SideAction::None)
    } else if view.can_connect || view.start_on_connect {
        let side = if view.can_unblock {
            SideAction::Unblock
        } else if view.route_ready {
            SideAction::Route
        } else {
            SideAction::None
        };
        button(Key::ButtonConnect, MainAction::Connect, side)
    } else {
        button(Key::ButtonConnect, MainAction::None, unblock)
    }
}

/// Smallest range in which `new` differs from `old`: (start, rows of `old` removed, rows of `new` inserted),
/// with the common beginning and end left untouched. `None` if both are equal. Replacing only this range
/// keeps the scroll position of list views when a star, an arrow or the selection changes.
pub fn splice<T: PartialEq>(old: &[T], new: &[T]) -> Option<(usize, usize, usize)> {
    if old == new {
        return None;
    }
    let start = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    // Beginning and end never overlap: the end is only searched behind the common beginning.
    let end = old[start..]
        .iter()
        .rev()
        .zip(new[start..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    Some((start, old.len() - start - end, new.len() - start - end))
}

/// Height of a row of the location list in `ui/locations.slint`.
pub const ROW_HEIGHT: f32 = 52.0;

/// Scroll position (`content-y` of the list view, 0 or negative) after the rows in `changed` (from
/// [`splice`]) were replaced, so that the visible rows stay where they are: if the change starts above the
/// first visible row, the list moves by the rows inserted or removed there; a change inside the visible
/// area moves nothing. Never above 0 (the start of the list).
pub fn scroll_after(changed: Option<(usize, usize, usize)>, scroll: f32) -> f32 {
    let Some((start, removed, inserted)) = changed else {
        return scroll.min(0.0);
    };
    #[expect(clippy::cast_precision_loss, reason = "a few hundred rows at most")]
    let (start, removed, inserted) = (start as f32, removed as f32, inserted as f32);
    let shift = if start * ROW_HEIGHT < -scroll {
        (removed - inserted) * ROW_HEIGHT
    } else {
        0.0
    };
    (scroll + shift).min(0.0)
}

/// Kind of a row in the location list. All rows have the same height in the user interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    /// "Favorites" or "All locations".
    Heading,
    /// A selectable location in one row: country with one server, streaming server, favourite.
    Location,
    /// Country with several locations; with "Automatic" it stands for it.
    Country,
    /// Location of an unfolded country, indented, without the country name.
    Child,
    /// Favourite that is not in the list (any more).
    Missing,
    /// Favourite while the server list is still loading.
    Loading,
    /// "Best location": logo instead of a flag, Celata chooses the location when connecting (feature 013).
    Best,
}

impl ItemKind {
    /// Value for the user interface.
    pub fn as_str(self) -> &'static str {
        match self {
            ItemKind::Heading => "heading",
            ItemKind::Location => "location",
            ItemKind::Country => "country",
            ItemKind::Child => "child",
            ItemKind::Missing => "missing",
            ItemKind::Loading => "loading",
            ItemKind::Best => "best",
        }
    }
}

/// A row of the location list, with texts in the language in force.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocationItem {
    pub kind: ItemKind,
    /// Country code for the flag; empty for headings.
    pub code: String,
    /// Country name (bold), the text of a heading, or empty.
    pub country: String,
    /// Place, "Automatic, hide.me chooses the location", "Streaming", the place name of an unfolded
    /// location, or the host name of a favourite that is missing or loading.
    pub place: String,
    /// Host for connecting; empty if the row cannot be selected.
    pub host: String,
    /// Host the star switches; empty: the row has no star.
    pub star_host: String,
    pub free: bool,
    /// Text for the selected location, for example "Germany, Berlin".
    pub choice: String,
    /// The star is yellow.
    pub favorite: bool,
    /// The row has an arrow.
    pub expandable: bool,
    /// The arrow points up.
    pub expanded: bool,
    /// Folded, and a location below it is selected.
    pub holds_selected: bool,
    /// Folded, and a location below it is connected.
    pub holds_connected: bool,
    /// Streaming server.
    pub streaming: bool,
}

impl LocationItem {
    fn heading(text: &str) -> Self {
        Self {
            kind: ItemKind::Heading,
            code: String::new(),
            country: text.to_owned(),
            place: String::new(),
            host: String::new(),
            star_host: String::new(),
            free: false,
            choice: String::new(),
            favorite: false,
            expandable: false,
            expanded: false,
            holds_selected: false,
            holds_connected: false,
            streaming: false,
        }
    }

    /// Favourite that cannot be selected right now: host name, only the star works.
    fn unavailable(host: &str, state: favorites::State) -> Self {
        Self {
            kind: if state == favorites::State::Loading {
                ItemKind::Loading
            } else {
                ItemKind::Missing
            },
            place: host.to_owned(),
            star_host: host.to_owned(),
            favorite: true,
            ..Self::heading("")
        }
    }
}

/// What the location lists are built from.
#[derive(Debug, Clone, Copy)]
pub struct ListContext<'a> {
    /// Loaded server list (built country names that failed are already removed).
    pub countries: &'a [Country],
    pub language: Language,
    pub query: &'a str,
    /// "Free only" in the tab "Standard"; never applies to the favourites.
    pub free_only: bool,
    /// Unfolded countries (codes), only for this session.
    pub expanded: &'a BTreeSet<String>,
    /// Selected host, empty without a selection.
    pub selected: &'a str,
    pub connected: Option<&'a str>,
    /// Favourites, newest first.
    pub favorites: &'a [String],
    /// The server list is still loading.
    pub loading: bool,
    /// Host "Best location" chose last in this session (`best::Memory::host`), for "Chosen: …".
    pub best_host: Option<&'a str>,
}

fn choice(country: &Country, server: &servers::Server, language: Language) -> String {
    i18n::fill(
        i18n::text(language, Key::LocationChoice),
        &[country.name(language), server.name(language)],
    )
}

fn streaming_choice(server: &StreamingServer, language: Language) -> String {
    Phrase::fill(
        Key::LocationChoice,
        [Phrase::key(Key::StreamingPlace), server.country()],
    )
    .render(language)
}

/// Does a search term match a host name? Used for favourites that are not in the list.
fn host_matches(host: &str, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    query.is_empty() || host.contains(&query)
}

/// Row of a server in "All locations" or among the favourites. `place` is the text below the country.
fn server_item(
    kind: ItemKind,
    country: &Country,
    server: &servers::Server,
    place: String,
    ctx: &ListContext,
) -> LocationItem {
    LocationItem {
        kind,
        code: country.code.clone(),
        country: if kind == ItemKind::Child {
            String::new()
        } else {
            country.name(ctx.language)
        },
        place,
        host: server.host.clone(),
        star_host: server.host.clone(),
        free: server.free,
        choice: choice(country, server, ctx.language),
        favorite: ctx.favorites.contains(&server.host),
        expandable: false,
        expanded: false,
        holds_selected: false,
        holds_connected: false,
        streaming: false,
    }
}

/// Text below the country in a row of its own: "Automatic, hide.me chooses the location" or the place.
fn place_of(server: &servers::Server, language: Language) -> String {
    if server.automatic {
        i18n::text(language, Key::AutomaticPlace).to_owned()
    } else {
        server.name(language)
    }
}

/// Rows of one country in "All locations": one row if the country has a single server (after "Free
/// only"), otherwise a country row (for "Automatic" if it is among the servers shown) and, when unfolded,
/// its other locations in the order of the list. `single`: the country has one server without the search.
fn country_items(country: &Country, by_country: bool, single: bool, ctx: &ListContext, rows: &mut Vec<LocationItem>) {
    if let (true, [server]) = (single, country.servers.as_slice()) {
        rows.push(server_item(
            ItemKind::Location,
            country,
            server,
            place_of(server, ctx.language),
            ctx,
        ));
        return;
    }
    // During a search that only matched places, the country is unfolded and shows just those.
    let expanded = ctx.expanded.contains(&country.code) || (!ctx.query.trim().is_empty() && !by_country);
    let automatic = country.servers.iter().find(|server| server.automatic);
    let children: Vec<&servers::Server> = country.servers.iter().filter(|server| !server.automatic).collect();
    let mut row = match automatic {
        Some(server) => server_item(
            ItemKind::Country,
            country,
            server,
            i18n::text(ctx.language, Key::AutomaticPlace).to_owned(),
            ctx,
        ),
        None => LocationItem {
            kind: ItemKind::Country,
            code: country.code.clone(),
            country: country.name(ctx.language),
            ..LocationItem::heading("")
        },
    };
    row.expandable = true;
    row.expanded = expanded;
    if !expanded {
        row.holds_selected = children.iter().any(|server| server.host == ctx.selected);
        row.holds_connected = children
            .iter()
            .any(|server| Some(server.host.as_str()) == ctx.connected);
    }
    rows.push(row);
    if expanded {
        rows.extend(
            children
                .into_iter()
                .map(|server| server_item(ItemKind::Child, country, server, server.name(ctx.language), ctx)),
        );
    }
}

/// Row "Best location". Below the name the hint, or "Chosen: Germany, Berlin" while connected to the host
/// "Best location" chose; never marked as connected itself, that is the row of the real location.
fn best_item(ctx: &ListContext) -> LocationItem {
    let name = i18n::text(ctx.language, Key::BestName).to_owned();
    let chosen = ctx
        .best_host
        .filter(|host| {
            ctx.connected
                .is_some_and(|connected| connected.eq_ignore_ascii_case(host))
        })
        .and_then(|host| location_of(host, ctx.countries, ctx.language));
    LocationItem {
        kind: ItemKind::Best,
        country: name.clone(),
        place: match chosen {
            Some((_, label)) => i18n::fill(i18n::text(ctx.language, Key::BestChosen), &[label]),
            None => i18n::text(ctx.language, Key::BestHint).to_owned(),
        },
        host: best::BEST.to_owned(),
        star_host: best::BEST.to_owned(),
        choice: name,
        favorite: ctx.favorites.iter().any(|favorite| favorite == best::BEST),
        ..LocationItem::heading("")
    }
}

/// Rows of the tab "Standard": "Favorites" with the standard favourites (newest first), then "All
/// locations" with the countries. Order of the steps: search, "Free only", grouping by country, sorting
/// by the language. A heading without rows below it is left out; without any hit the list is empty.
pub fn location_items(ctx: &ListContext) -> Vec<LocationItem> {
    let found = servers::search_marked(ctx.countries, ctx.query);
    let found_hosts: BTreeSet<&str> = found
        .iter()
        .flat_map(|found| &found.country.servers)
        .map(|server| server.host.as_str())
        .collect();
    let mut favorites = Vec::new();
    for host in ctx.favorites {
        if favorites::tab_of(host) != favorites::Tab::Standard {
            continue;
        }
        let state = favorites::resolve(host, ctx.countries, ctx.loading, &[]);
        if host == best::BEST {
            if best::matches(ctx.query) {
                favorites.push(if state == favorites::State::Loading {
                    // Loading like the other favourites, but with its name instead of the mark.
                    LocationItem {
                        place: i18n::text(ctx.language, Key::BestName).to_owned(),
                        ..LocationItem::unavailable(host, state)
                    }
                } else {
                    best_item(ctx)
                });
            }
            continue;
        }
        let known = ctx.countries.iter().find_map(|country| {
            country
                .servers
                .iter()
                .find(|server| server.host == *host)
                .map(|server| (country, server))
        });
        match known {
            Some((country, server)) if state == favorites::State::Available => {
                if found_hosts.contains(host.as_str()) {
                    favorites.push(server_item(
                        ItemKind::Location,
                        country,
                        server,
                        server.name(ctx.language),
                        ctx,
                    ));
                }
            }
            _ => {
                if host_matches(host, ctx.query) {
                    favorites.push(LocationItem::unavailable(host, state));
                }
            }
        }
    }
    let mut countries: Vec<(Country, bool)> = found
        .into_iter()
        .map(|found| (found.country, found.by_country))
        .collect();
    if ctx.free_only {
        for (country, _) in &mut countries {
            country.servers.retain(|server| server.free);
        }
        countries.retain(|(country, _)| !country.servers.is_empty());
    }
    countries.sort_by_key(|(country, _)| names::sort_key(&country.name(ctx.language)));
    // Whether a country has one server is decided without the search: a search that matches one place of a
    // country with several shows it unfolded below its country row.
    let shown_without_search = |code: &str| {
        ctx.countries
            .iter()
            .find(|country| country.code == code)
            .map_or(0, |country| {
                country
                    .servers
                    .iter()
                    .filter(|server| !ctx.free_only || server.free)
                    .count()
            })
    };
    let mut all = Vec::new();
    // "Best location" first, independent of the sorting, as soon as a list is there.
    if !ctx.countries.is_empty() && best::matches(ctx.query) {
        all.push(best_item(ctx));
    }
    for (country, by_country) in &countries {
        let single = shown_without_search(&country.code) <= 1;
        country_items(country, *by_country, single, ctx, &mut all);
    }
    with_headings(favorites, all, ctx.query, ctx.language)
}

/// "Favorites" in front of the favourites, only if there are any; "All locations" always, also while
/// the server list is loading or could not be loaded. Only a search without hits below it leaves it out.
fn with_headings(
    favorites: Vec<LocationItem>,
    all: Vec<LocationItem>,
    query: &str,
    language: Language,
) -> Vec<LocationItem> {
    let mut rows = Vec::with_capacity(favorites.len() + all.len() + 2);
    if !favorites.is_empty() {
        rows.push(LocationItem::heading(i18n::text(language, Key::ListHeadingFavorites)));
        rows.extend(favorites);
    }
    if !all.is_empty() || query.trim().is_empty() {
        rows.push(LocationItem::heading(i18n::text(language, Key::ListHeadingAll)));
        rows.extend(all);
    }
    rows
}

fn streaming_item(server: &StreamingServer, ctx: &ListContext) -> LocationItem {
    LocationItem {
        kind: ItemKind::Location,
        code: server.code.clone(),
        country: server.name(ctx.language),
        place: i18n::text(ctx.language, Key::StreamingPlace).to_owned(),
        host: server.host.clone(),
        star_host: server.host.clone(),
        free: false,
        choice: streaming_choice(server, ctx.language),
        favorite: ctx.favorites.contains(&server.host),
        expandable: false,
        expanded: false,
        holds_selected: false,
        holds_connected: false,
        streaming: true,
    }
}

/// Rows of the tab "Streaming" without grouping (one server per country): "Favorites" with the streaming
/// favourites, then "All locations", sorted by the country name in the language. `servers` are the
/// visible streaming servers; the search term comes from `ctx`.
pub fn streaming_items(servers: &[StreamingServer], ctx: &ListContext) -> Vec<LocationItem> {
    let mut favorites = Vec::new();
    for host in ctx.favorites {
        if favorites::tab_of(host) != favorites::Tab::Streaming {
            continue;
        }
        match servers.iter().find(|server| server.host == *host) {
            Some(server) => {
                if !streaming::search(std::slice::from_ref(server), ctx.query).is_empty() {
                    favorites.push(streaming_item(server, ctx));
                }
            }
            None => {
                if host_matches(host, ctx.query) {
                    favorites.push(LocationItem::unavailable(host, favorites::State::Missing));
                }
            }
        }
    }
    let mut all: Vec<LocationItem> = streaming::search(servers, ctx.query)
        .iter()
        .map(|server| streaming_item(server, ctx))
        .collect();
    all.sort_by_key(|row| names::sort_key(&row.country));
    with_headings(favorites, all, ctx.query, ctx.language)
}

/// A row of "Favorite locations" in the overview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FavoriteItem {
    /// Country code for the flag; empty if missing or loading.
    pub code: String,
    /// "Germany, Frankfurt #1", for a streaming server only the country ("Japan"; the play symbol shows that it
    /// is a streaming server), or the host name.
    pub label: String,
    /// Host for connecting; empty if it cannot be selected.
    pub host: String,
    /// Host the star switches (also when missing).
    pub star_host: String,
    pub state: favorites::State,
    /// Streaming server: small play symbol.
    pub streaming: bool,
    pub connected: bool,
    /// Text for the selected location: like `label`, for a streaming server "Streaming, Japan".
    pub choice: String,
    /// "Best location": logo instead of a flag.
    pub best: bool,
}

/// All favourites of both tabs in the stored order (newest first), without search and without "Free
/// only".
pub fn favorite_items(
    favorites: &[String],
    countries: &[Country],
    streaming_visible: &[StreamingServer],
    loading: bool,
    connected: Option<&str>,
    language: Language,
) -> Vec<FavoriteItem> {
    favorites
        .iter()
        .map(|host| {
            let state = favorites::resolve(host, countries, loading, streaming_visible);
            if host == best::BEST {
                let name = i18n::text(language, Key::BestName).to_owned();
                return FavoriteItem {
                    code: String::new(),
                    label: name.clone(),
                    // While loading it cannot be selected, like the other favourites.
                    host: if state == favorites::State::Available {
                        host.clone()
                    } else {
                        String::new()
                    },
                    star_host: host.clone(),
                    state,
                    streaming: false,
                    connected: false,
                    choice: name,
                    best: true,
                };
            }
            let found = (state == favorites::State::Available)
                .then(|| location_of(host, countries, language))
                .flatten();
            let streaming = favorites::tab_of(host) == favorites::Tab::Streaming;
            match found {
                Some((code, choice)) => FavoriteItem {
                    // The play symbol stands for "Streaming"; the card "Location" keeps the full text.
                    label: if streaming {
                        streaming::all()
                            .iter()
                            .find(|server| server.host == *host)
                            .map_or_else(|| choice.clone(), |server| server.name(language))
                    } else {
                        choice.clone()
                    },
                    code,
                    choice,
                    host: host.clone(),
                    star_host: host.clone(),
                    state,
                    streaming,
                    connected: connected == Some(host.as_str()),
                    best: false,
                },
                None => FavoriteItem {
                    code: String::new(),
                    label: host.clone(),
                    host: String::new(),
                    star_host: host.clone(),
                    // Available but not found cannot happen; it is shown as missing then.
                    state: if state == favorites::State::Loading {
                        state
                    } else {
                        favorites::State::Missing
                    },
                    streaming,
                    connected: false,
                    choice: host.clone(),
                    best: false,
                },
            }
        })
        .collect()
}

/// Country code and text ("Germany, Frankfurt" or "Streaming, Japan") for a host, otherwise `None`.
pub fn location_of(host: &str, countries: &[Country], language: Language) -> Option<(String, String)> {
    let found = countries.iter().find_map(|country| {
        country
            .servers
            .iter()
            .find(|server| server.host.eq_ignore_ascii_case(host))
            .map(|server| (country.code.clone(), choice(country, server, language)))
    });
    found.or_else(|| {
        streaming::all()
            .into_iter()
            .find(|server| server.host.eq_ignore_ascii_case(host))
            .map(|server| (server.code.clone(), streaming_choice(&server, language)))
    })
}

/// Location that the overview shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverviewLocation {
    /// Country code for the flag, `None` without a flag.
    pub code: Option<String>,
    pub label: String,
    /// "Best location": the logo instead of the flag.
    pub best: bool,
}

/// Location of a host: known from the list or the streaming table, otherwise its name.
fn host_location(host: &str, fallback: &str, countries: &[Country], language: Language) -> OverviewLocation {
    match location_of(host, countries, language) {
        Some((code, label)) => OverviewLocation {
            code: Some(code),
            label,
            best: false,
        },
        None => OverviewLocation {
            code: None,
            label: fallback.to_owned(),
            best: false,
        },
    }
}

/// Location of the overview. Precedence: the connected server, then the target of the running connection
/// attempt (`connect_target`), then the selection. A selection `best` or an empty one is "Best location"
///, also while the list is loading.
pub fn overview_location(
    connected_host: &str,
    connect_target: Option<&str>,
    selected_host: &str,
    selected_name: &str,
    countries: &[Country],
    language: Language,
) -> OverviewLocation {
    if !connected_host.is_empty() {
        let fallback = streaming::server_label(connected_host).render(language);
        return host_location(connected_host, &fallback, countries, language);
    }
    if let Some(target) = connect_target.filter(|target| !target.is_empty()) {
        let fallback = streaming::server_label(target).render(language);
        return host_location(target, &fallback, countries, language);
    }
    if best::is_best(selected_host) {
        return OverviewLocation {
            code: None,
            label: i18n::text(language, Key::BestName).to_owned(),
            best: true,
        };
    }
    // The selected name follows the language if the host is known; otherwise the text from the list stays.
    host_location(selected_host, selected_name, countries, language)
}

/// Target while connecting or switching for the status card: "Germany, Berlin (berlin.hideservers.net)", a
/// streaming server as "Streaming Japan (streaming-jp.hideservers.net)", otherwise the host name.
pub fn target_label(host: &str, countries: &[Country], language: Language) -> String {
    if streaming::describe_host(host).is_some() {
        return streaming::server_label(host).render(language);
    }
    match location_of(host, countries, language) {
        Some((_, label)) => i18n::fill(i18n::text(language, Key::TargetLabel), &[label, host.to_owned()]),
        None => host.to_owned(),
    }
}

/// The same for narrow places such as the sidebar, without the host name: "Germany, Berlin", "Streaming
/// Japan"; an unknown host keeps its name.
pub fn target_short(host: &str, countries: &[Country], language: Language) -> String {
    if streaming::describe_host(host).is_some() {
        return streaming::short_label(host).render(language);
    }
    location_of(host, countries, language).map_or_else(|| host.to_owned(), |(_, label)| label)
}

// ---------------------------------------------------------------------------------------------------
// Connection details and own IP address
// ---------------------------------------------------------------------------------------------------

/// Days since 1970-01-01 for a date (algorithm "days from civil" by Howard Hinnant).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_index = (month + 9) % 12;
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// Reads a time stamp such as `2026-11-09T12:34:56.123456789+01:00` or `…Z` (RFC 3339, as Go writes
/// it). Without new crates; fractions of seconds are skipped.
pub fn parse_rfc3339(text: &str) -> Option<SystemTime> {
    let bytes = text.as_bytes();
    if bytes.len() < 20 || bytes[4] != b'-' || bytes[7] != b'-' || !matches!(bytes[10], b'T' | b't') {
        return None;
    }
    if bytes[13] != b':' || bytes[16] != b':' {
        return None;
    }
    let number = |range: std::ops::Range<usize>| -> Option<i64> {
        let part = text.get(range)?;
        part.bytes()
            .all(|byte| byte.is_ascii_digit())
            .then(|| part.parse().ok())?
    };
    let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
    let (hour, minute, second) = (number(11..13)?, number(14..16)?, number(17..19)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let mut rest = &text[19..];
    if let Some(fraction) = rest.strip_prefix('.') {
        let digits = fraction.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            return None;
        }
        rest = &fraction[digits..];
    }
    let offset = match rest {
        "Z" | "z" => 0,
        _ => {
            let sign = match rest.as_bytes().first()? {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let zone = rest.get(1..)?;
            if zone.len() != 5 || zone.as_bytes()[2] != b':' {
                return None;
            }
            let hours: i64 = zone[0..2].parse().ok()?;
            let minutes: i64 = zone[3..5].parse().ok()?;
            sign * (hours * 3600 + minutes * 60)
        }
    };
    let seconds = days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second - offset;
    let seconds = u64::try_from(seconds).ok()?;
    UNIX_EPOCH.checked_add(Duration::from_secs(seconds))
}

/// Duration in the notation of the language: "1 h 4 min", "3 min", "less than 1 min" or the German
/// counterparts.
pub fn duration_text(duration: Duration, language: Language) -> String {
    let minutes = duration.as_secs() / 60;
    let number = |value: u64| value.to_string();
    match (minutes / 60, minutes % 60) {
        (0, 0) => i18n::text(language, Key::DurationUnderMinute).to_owned(),
        (0, minutes) => i18n::fill(i18n::text(language, Key::DurationMinutes), &[number(minutes)]),
        (hours, minutes) => i18n::fill(
            i18n::text(language, Key::DurationHoursMinutes),
            &[number(hours), number(minutes)],
        ),
    }
}

/// Amount of data in decimal units with the decimal separator of the language: "999 B", "1.5 kB",
/// "12.3 MB", "2.0 GB" or "1,5 kB".
pub fn bytes_text(bytes: u64, language: Language) -> String {
    const UNITS: [&str; 4] = ["kB", "MB", "GB", "TB"];
    if bytes < 1000 {
        return format!("{bytes} B");
    }
    // Calculate in tenths, so that nothing jumps over the unit through rounding.
    let mut tenths = bytes / 100;
    let mut unit = 0;
    while tenths >= 10_000 && unit + 1 < UNITS.len() {
        tenths /= 1000;
        unit += 1;
    }
    format!(
        "{}{}{} {}",
        tenths / 10,
        language.decimal_separator(),
        tenths % 10,
        UNITS[unit]
    )
}

/// Address without prefix for single addresses (`10.0.0.2/32` → `10.0.0.2`).
fn address_text(address: &str) -> &str {
    address
        .strip_suffix("/32")
        .or_else(|| address.strip_suffix("/128"))
        .unwrap_or(address)
}

/// Splits addresses into IPv4 and IPv6, several of one kind each in a line of their own; single addresses
/// without prefix.
fn split_families(list: &[String]) -> (String, String) {
    let (mut v4, mut v6) = (Vec::new(), Vec::new());
    for item in list {
        let address = address_text(item);
        let ip = address.split('/').next().unwrap_or(address);
        match ip.parse::<std::net::IpAddr>() {
            Ok(std::net::IpAddr::V4(_)) => v4.push(address),
            Ok(std::net::IpAddr::V6(_)) => v6.push(address),
            Err(_) => {}
        }
    }
    (v4.join("\n"), v6.join("\n"))
}

/// Connection details for the overview; empty except in "Connected". Addresses separated into IPv4
/// and IPv6, so that the overview can show them in two columns.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConnectionDetails {
    pub duration: String,
    pub received: String,
    pub sent: String,
    pub address_v4: String,
    pub address_v6: String,
    pub dns_v4: String,
    pub dns_v6: String,
}

impl ConnectionDetails {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Details from the state of the service that was read last, only in "Connected".
pub fn connection_details(
    display: Option<&DisplayState>,
    state: Option<&ServiceState>,
    now: SystemTime,
    language: Language,
) -> ConnectionDetails {
    let (Some(DisplayState::Connected { .. }), Some(state)) = (display, state) else {
        return ConnectionDetails::default();
    };
    service_details(state, now, language)
}

/// Details from a state with code `connected`; otherwise empty.
pub fn service_details(state: &ServiceState, now: SystemTime, language: Language) -> ConnectionDetails {
    if state.code != "connected" {
        return ConnectionDetails::default();
    }
    let duration = state
        .since
        .as_deref()
        .and_then(parse_rfc3339)
        .map(|since| duration_text(now.duration_since(since).unwrap_or_default(), language))
        .unwrap_or_default();
    let (address_v4, address_v6) = split_families(&state.addresses);
    let (dns_v4, dns_v6) = split_families(&state.dns);
    ConnectionDetails {
        duration,
        received: state.rx.map(|bytes| bytes_text(bytes, language)).unwrap_or_default(),
        sent: state.tx.map(|bytes| bytes_text(bytes, language)).unwrap_or_default(),
        address_v4,
        address_v6,
        dns_v4,
        dns_v6,
    }
}

/// How long Celata waits for the public address at most.
pub const EXTERNAL_IP_TIMEOUT: Duration = Duration::from_secs(10);
/// After a failure Celata asks again only after this time.
pub const EXTERNAL_IP_RETRY: Duration = Duration::from_secs(30);

/// State of the public address. Celata queries it itself as soon as "Connected" or "Not connected" is
/// reached (instead of a button). Never in "Internet blocked": the query would need exception routes
/// that bypass the block; the overview then shows "Internet blocked" in red.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ExternalIp {
    #[default]
    Idle,
    Asking {
        since: Instant,
    },
    Known {
        v4: Option<String>,
        v6: Option<String>,
    },
    Failed {
        at: Instant,
    },
}

impl ExternalIp {
    /// Accepts the report `external ips`, but only while a query is running.
    pub fn answer(&mut self, v4: Option<&str>, v6: Option<&str>, now: Instant) {
        if !matches!(self, Self::Asking { .. }) {
            return;
        }
        let clean = |ip: Option<&str>| {
            ip.map(str::trim)
                .filter(|ip| ip.parse::<std::net::IpAddr>().is_ok())
                .map(str::to_owned)
        };
        let (v4, v6) = (clean(v4), clean(v6));
        *self = if v4.is_none() && v6.is_none() {
            Self::Failed { at: now }
        } else {
            Self::Known { v4, v6 }
        };
    }

    /// After 10 seconds without a report the query counts as failed.
    pub fn expire(&mut self, now: Instant) -> bool {
        if let Self::Asking { since } = self
            && now.saturating_duration_since(*since) >= EXTERNAL_IP_TIMEOUT
        {
            *self = Self::Failed { at: now };
            return true;
        }
        false
    }

    /// Should it be queried (again) now? Never queried yet, or 30 seconds after a failure.
    pub fn due(&self, now: Instant) -> bool {
        match self {
            Self::Idle => true,
            Self::Failed { at } => now.saturating_duration_since(*at) >= EXTERNAL_IP_RETRY,
            Self::Asking { .. } | Self::Known { .. } => false,
        }
    }

    /// Display in the column IPv4 or IPv6: the address, "–" without an address of this kind, otherwise the state.
    pub fn column(&self, v6: bool, language: Language) -> String {
        match self {
            Self::Idle => String::new(),
            Self::Asking { .. } => i18n::text(language, Key::ExternalIpAsking).to_owned(),
            Self::Failed { .. } => i18n::text(language, Key::ExternalIpFailed).to_owned(),
            Self::Known { v4: four, v6: six } => {
                let ip = if v6 { six } else { four };
                ip.clone().unwrap_or_else(|| "–".to_owned())
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// Menu of the tray icon
// ---------------------------------------------------------------------------------------------------

/// Put in front of the connected location in the menu. Slint passes no check marks to the system tray under
/// Linux; once it does, `checked` is set already and this mark can go.
pub const CURRENT_MARK: &str = "✓ ";

/// Number of alternating sections of the location menu (`menu_slots`); `ui/tray.slint` has as many.
pub const MENU_SLOTS: usize = 12;

/// An entry of the menu with its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuEntry {
    pub label: String,
    pub enabled: bool,
}

/// A location in the menu: favourite, place of a country, streaming server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuPlace {
    pub label: String,
    /// Host chosen with this entry; `best` for "Best location".
    pub host: String,
    pub enabled: bool,
    /// The connected location; its text then starts with [`CURRENT_MARK`].
    pub current: bool,
}

/// A country in the menu. Without `places` it is one entry (`host` is its only location); with `places` it is
/// a submenu whose first entry is "Automatic" (if the country has one) followed by the places.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuCountry {
    pub label: String,
    /// Country code; only used to sort the country under its continent.
    pub code: String,
    /// Host of the single location, or of "Automatic"; empty for a country without "Automatic".
    pub host: String,
    pub enabled: bool,
    pub current: bool,
    pub places: Vec<MenuPlace>,
}

/// A continent in the menu: a submenu with its countries in the order of the tab "Standard".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuContinent {
    pub continent: Continent,
    /// Name in the language in force; starts with [`CURRENT_MARK`] when `current`.
    pub label: String,
    /// The connected location is one of its countries or places.
    pub current: bool,
    pub countries: Vec<MenuCountry>,
}

/// Content of the submenu "Locations" apart from "Best location" and "Streaming".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuLocations {
    /// The server list is loading: only a note.
    Loading,
    /// No server list: only a note.
    Missing,
    /// Continents in a fixed order (empty ones are left out), each with its countries in the order of the
    /// tab "Standard".
    Ready(Vec<MenuContinent>),
}

/// State of the server list for the menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListState {
    Loading,
    Missing,
    Ready,
}

/// Everything the menu of the tray icon shows, with texts in the language in force. Never a host name or an
/// address: the menu can be read by every program of the session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayMenu {
    /// Short state like the sidebar.
    pub status: String,
    /// Text and effect of the main button.
    pub main: MenuEntry,
    /// "Connect to best location" and "Best location" in "Locations" can be chosen.
    pub best_enabled: bool,
    /// "Block internet" or "Unblock internet", only when the red area of the main button offers it.
    pub side: Option<MenuEntry>,
    /// Favourites in the order and with the names of "Favorite locations"; empty: "No favorites".
    pub favorites: Vec<MenuPlace>,
    /// "Add current location to favorites" or "Remove current location from favorites".
    pub favorite_toggle: MenuEntry,
    pub locations: MenuLocations,
    /// Streaming servers, sorted like the tab "Streaming".
    pub streaming: Vec<MenuPlace>,
    /// "Celata 0.9.0 is available".
    pub update: Option<String>,
}

/// What the menu is built from.
#[derive(Debug, Clone, Copy)]
pub struct TrayContext<'a> {
    pub display: Option<&'a DisplayState>,
    pub view: &'a StatusView,
    /// The setting "Kill switch" is on; without it "Block internet" is not offered, as in the window.
    pub kill_switch: bool,
    /// Loaded server list (built country names that failed are already removed).
    pub countries: &'a [Country],
    /// Visible streaming servers.
    pub streaming: &'a [StreamingServer],
    pub favorites: &'a [String],
    pub list: ListState,
    pub free_only: bool,
    /// Selected location in the window; empty counts as "Best location".
    pub selected: &'a str,
    pub language: Language,
    /// Newer version, already as text "<a>.<b>.<c>".
    pub update: Option<&'a str>,
}

/// The service answers: connecting, "Best location" and the main button can work.
fn service_reachable(display: Option<&DisplayState>) -> bool {
    display.is_some_and(DisplayState::service_reachable)
}

/// Host of the established connection, otherwise `None`.
fn connected_host<'a>(display: Option<&DisplayState>, view: &'a StatusView) -> Option<&'a str> {
    (matches!(display, Some(DisplayState::Connected { .. })) && !view.connected_host.is_empty())
        .then_some(view.connected_host.as_str())
}

/// Text with the mark of the connected location in front.
fn marked(label: String, current: bool) -> String {
    if current {
        format!("{CURRENT_MARK}{label}")
    } else {
        label
    }
}

/// Location the entry "Add/Remove current location" refers to: the connected host, otherwise the selection
/// in the window, "Best location" without one.
pub fn current_location(display: Option<&DisplayState>, view: &StatusView, selected: &str) -> String {
    match connected_host(display, view) {
        Some(host) => host.to_owned(),
        None if selected.is_empty() => best::BEST.to_owned(),
        None => selected.to_owned(),
    }
}

/// Builds the menu from the same parts as main button, sidebar, "Favorite locations" and the location list.
pub fn tray_menu(ctx: &TrayContext) -> TrayMenu {
    let language = ctx.language;
    let button = main_button(ctx.display, ctx.view);
    // "Block internet" only with the setting "Kill switch" on, like the red area of the window.
    let offered = match button.side {
        SideAction::Unblock => true,
        SideAction::Route => ctx.kill_switch,
        SideAction::None => false,
    };
    let side = button.side_label.filter(|_| offered).map(|label| MenuEntry {
        label: i18n::text(language, label).to_owned(),
        enabled: true,
    });
    let connected = connected_host(ctx.display, ctx.view);
    let is_current = |host: &str| connected.is_some_and(|connected| connected.eq_ignore_ascii_case(host));
    let target = current_location(ctx.display, ctx.view, ctx.selected);
    // Locations, streaming servers and favourites connect: without an answering service they cannot be chosen,
    // unless connecting starts the service first.
    let reachable = service_reachable(ctx.display) || ctx.view.start_on_connect;
    let favorite_toggle = MenuEntry {
        label: i18n::text(
            language,
            if ctx
                .favorites
                .iter()
                .any(|favorite| favorite.eq_ignore_ascii_case(&target))
            {
                Key::TrayFavoriteRemove
            } else {
                Key::TrayFavoriteAdd
            },
        )
        .to_owned(),
        enabled: true,
    };
    let ready = ctx.list == ListState::Ready && !ctx.countries.is_empty();
    TrayMenu {
        status: crate::trayicon::short_state(ctx.view, ctx.countries, language),
        main: MenuEntry {
            label: i18n::text(language, button.label).to_owned(),
            enabled: button.action != MainAction::None,
        },
        best_enabled: reachable && ready,
        side,
        favorites: menu_favorites(ctx, reachable),
        favorite_toggle,
        locations: if ready {
            MenuLocations::Ready(menu_continents(
                menu_countries(ctx, &is_current, reachable),
                &continent::assign(ctx.countries),
                ctx.language,
            ))
        } else if ctx.list == ListState::Loading {
            MenuLocations::Loading
        } else {
            MenuLocations::Missing
        },
        streaming: menu_streaming(ctx, &is_current, reachable),
        update: ctx
            .update
            .map(|version| i18n::fill(i18n::text(language, Key::UpdateAvailable), &[version.to_owned()])),
    }
}

/// Favourites like "Favorite locations", without the connected mark. Not selectable: missing or loading
/// (text without host name), a paid location while "Free only" is on, and all while the service does not answer.
fn menu_favorites(ctx: &TrayContext, reachable: bool) -> Vec<MenuPlace> {
    let loading = ctx.list == ListState::Loading;
    favorite_items(ctx.favorites, ctx.countries, ctx.streaming, loading, None, ctx.language)
        .into_iter()
        .map(|item| {
            let label = match item.state {
                favorites::State::Available if item.streaming => i18n::fill(
                    i18n::text(ctx.language, Key::TrayStreamingPlace),
                    std::slice::from_ref(&item.label),
                ),
                favorites::State::Available if !item.host.is_empty() => item.label.clone(),
                favorites::State::Loading => i18n::text(ctx.language, Key::TrayFavoriteLoading).to_owned(),
                _ => i18n::text(ctx.language, Key::TrayFavoriteUnavailable).to_owned(),
            };
            let paid = ctx.free_only
                && ctx
                    .countries
                    .iter()
                    .flat_map(|country| &country.servers)
                    .any(|server| server.host == item.host && !server.free);
            MenuPlace {
                label,
                enabled: reachable && !item.host.is_empty() && !paid,
                host: item.host,
                current: false,
            }
        })
        .collect()
}

/// Countries of the tab "Standard" without search, all unfolded, with "Free only". Locations can only be chosen
/// while the service answers; the submenus of countries stay open.
fn menu_countries(ctx: &TrayContext, is_current: &dyn Fn(&str) -> bool, reachable: bool) -> Vec<MenuCountry> {
    let expanded: BTreeSet<String> = ctx.countries.iter().map(|country| country.code.clone()).collect();
    let list = ListContext {
        countries: ctx.countries,
        language: ctx.language,
        query: "",
        free_only: ctx.free_only,
        expanded: &expanded,
        selected: "",
        connected: None,
        favorites: &[],
        loading: false,
        best_host: None,
    };
    // Free locations carry the mark the list shows as a badge.
    let free = |label: String, free: bool| {
        if free {
            i18n::fill(i18n::text(ctx.language, Key::TrayFreePlace), &[label])
        } else {
            label
        }
    };
    let place = |label: String, host: &str| MenuPlace {
        label: marked(label, is_current(host)),
        host: host.to_owned(),
        enabled: reachable,
        current: is_current(host),
    };
    let mut countries: Vec<MenuCountry> = Vec::new();
    for row in location_items(&list) {
        match row.kind {
            ItemKind::Location => countries.push(MenuCountry {
                label: marked(free(row.country, row.free), is_current(&row.host)),
                code: row.code,
                current: is_current(&row.host),
                host: row.host,
                enabled: reachable,
                places: Vec::new(),
            }),
            ItemKind::Country => {
                let mut places = Vec::new();
                if !row.host.is_empty() {
                    places.push(place(
                        free(i18n::text(ctx.language, Key::TrayAutomatic).to_owned(), row.free),
                        &row.host,
                    ));
                }
                countries.push(MenuCountry {
                    label: row.country,
                    code: row.code,
                    host: row.host,
                    enabled: true,
                    current: false,
                    places,
                });
            }
            ItemKind::Child => {
                if let Some(country) = countries.last_mut() {
                    country.places.push(place(free(row.place, row.free), &row.host));
                }
            }
            ItemKind::Heading | ItemKind::Best | ItemKind::Missing | ItemKind::Loading => {}
        }
    }
    countries
}

/// Distributes the countries, in their order, over the continents in the fixed order of [`continent::ALL`];
/// a continent without a country is left out. A country without an entry in `assignment` (not possible after
/// `continent::assign`) goes to Europe, like the last step of the assignment.
pub fn menu_continents(
    countries: Vec<MenuCountry>,
    assignment: &BTreeMap<String, Continent>,
    language: Language,
) -> Vec<MenuContinent> {
    let mut groups: BTreeMap<Continent, Vec<MenuCountry>> = BTreeMap::new();
    for country in countries {
        let continent = assignment.get(&country.code).copied().unwrap_or(Continent::Europe);
        groups.entry(continent).or_default().push(country);
    }
    continent::ALL
        .iter()
        .filter_map(|continent| {
            let countries = groups.remove(continent)?;
            let current = countries
                .iter()
                .any(|country| country.current || country.places.iter().any(|place| place.current));
            Some(MenuContinent {
                continent: *continent,
                label: marked(continent.name(language).to_owned(), current),
                current,
                countries,
            })
        })
        .collect()
}

/// Streaming servers like the tab "Streaming" without search, named by their country; selectable only while the
/// service answers.
fn menu_streaming(ctx: &TrayContext, is_current: &dyn Fn(&str) -> bool, reachable: bool) -> Vec<MenuPlace> {
    let mut servers: Vec<MenuPlace> = ctx
        .streaming
        .iter()
        .map(|server| MenuPlace {
            label: marked(server.name(ctx.language), is_current(&server.host)),
            host: server.host.clone(),
            enabled: reachable,
            current: is_current(&server.host),
        })
        .collect();
    servers.sort_by_key(|place| names::sort_key(place.label.trim_start_matches(CURRENT_MARK)));
    servers
}

/// A section of the location menu: first countries with one location, then countries with several.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MenuSlot {
    pub singles: Vec<MenuCountry>,
    pub groups: Vec<MenuCountry>,
}

/// Splits the sorted countries into at most `count` alternating sections, so that the menu keeps the order of
/// the list: Slint builds an entry or a submenu per loop, never either of them in one loop. What does not fit
/// goes into the last section (nothing is lost; only there the order can differ).
pub fn menu_slots(countries: &[MenuCountry], count: usize) -> Vec<MenuSlot> {
    let mut slots: Vec<MenuSlot> = Vec::new();
    let mut current = MenuSlot::default();
    for country in countries {
        if country.places.is_empty() {
            // A single country after groups starts the next section, unless the last one is reached.
            if !current.groups.is_empty() && slots.len() + 1 < count.max(1) {
                slots.push(std::mem::take(&mut current));
            }
            current.singles.push(country.clone());
        } else {
            current.groups.push(country.clone());
        }
    }
    slots.push(current);
    slots
}

/// What was chosen in the menu. It carries no state: the decision is made in the state at the time of the
/// choice (`menu_decision`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuAction {
    ShowWindow,
    /// The entry of the main button.
    Main,
    /// "Connect to best location".
    Best,
    /// "Block internet" or "Unblock internet".
    Side,
    /// A location from "Favorites" or "Locations".
    Choose(String),
    ToggleFavorite,
    OpenUpdate,
    About,
    Quit,
}

/// What Celata does for a choice in the menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuDecision {
    /// Only bring the window back.
    BringBack,
    /// Connect, or switch while connected, like a double click in the list.
    Connect(String),
    /// The service is unreachable: start it first, then connect to `target` after its first answer.
    StartThenConnect {
        target: String,
    },
    /// Like "Disconnect" on the main button.
    Disconnect,
    /// "Block internet", without a question, like the red area.
    Route,
    /// Window back and the question before unblocking.
    AskUnblock,
    ToggleFavorite(String),
    OpenUpdate,
    /// Window back and the dialog "About Celata".
    ShowAbout,
    /// Like closing the window with the switch "Keep running after closing" off.
    Quit,
    Nothing,
}

/// State at the time of the choice.
#[derive(Debug, Clone, Copy)]
pub struct MenuState<'a> {
    pub display: Option<&'a DisplayState>,
    pub view: &'a StatusView,
    /// A question of the window is open (confirmation, closing, unblocking).
    pub dialog_open: bool,
    /// Selected location in the window; empty counts as "Best location".
    pub selected: &'a str,
    pub kill_switch: bool,
}

/// Decides a choice in the menu in the current state, as the window would: a question open in the window
/// comes first, and an entry that no longer fits does nothing.
pub fn menu_decision(action: &MenuAction, state: &MenuState) -> MenuDecision {
    if state.dialog_open && !matches!(action, MenuAction::ShowWindow | MenuAction::Quit) {
        return MenuDecision::BringBack;
    }
    let button = main_button(state.display, state.view);
    match action {
        MenuAction::ShowWindow => MenuDecision::BringBack,
        MenuAction::About => MenuDecision::ShowAbout,
        MenuAction::Quit => MenuDecision::Quit,
        MenuAction::OpenUpdate => MenuDecision::OpenUpdate,
        MenuAction::Main => match button.action {
            MainAction::Connect => {
                let target = if state.selected.is_empty() {
                    best::BEST
                } else {
                    state.selected
                };
                wish_decision(connect_wish(state.display, state.view, target))
            }
            MainAction::Disconnect => MenuDecision::Disconnect,
            MainAction::None => MenuDecision::Nothing,
        },
        // Connecting needs an answering service or one Celata may start; an entry chosen before that changed does
        // nothing.
        MenuAction::Best => wish_decision(connect_wish(state.display, state.view, best::BEST)),
        MenuAction::Choose(host) => wish_decision(connect_wish(state.display, state.view, host)),
        MenuAction::Side => match button.side {
            SideAction::Route if state.kill_switch => MenuDecision::Route,
            SideAction::Unblock => MenuDecision::AskUnblock,
            SideAction::Route | SideAction::None => MenuDecision::Nothing,
        },
        MenuAction::ToggleFavorite => {
            MenuDecision::ToggleFavorite(current_location(state.display, state.view, state.selected))
        }
    }
}

/// What a wish to connect (main button, double click, entry in the menu) does in the current state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectWish {
    /// The service answers: connect, or switch while connected.
    Connect(String),
    /// The service is unreachable and may be started: start it, then connect to the target.
    StartThenConnect(String),
    /// Neither is possible right now (for example while the service starts, or a reason against starting it is
    /// shown).
    Nothing,
}

/// Decides a wish to connect to `target` (a host or `best::BEST`).
pub fn connect_wish(display: Option<&DisplayState>, view: &StatusView, target: &str) -> ConnectWish {
    if view.start_on_connect {
        ConnectWish::StartThenConnect(target.to_owned())
    } else if service_reachable(display) {
        ConnectWish::Connect(target.to_owned())
    } else {
        ConnectWish::Nothing
    }
}

fn wish_decision(wish: ConnectWish) -> MenuDecision {
    match wish {
        ConnectWish::Connect(target) => MenuDecision::Connect(target),
        ConnectWish::StartThenConnect(target) => MenuDecision::StartThenConnect { target },
        ConnectWish::Nothing => MenuDecision::Nothing,
    }
}

/// Does a text name a host (`name.example`) or an IPv4 or IPv6 address? Numbers such as "1.5 GB" or a
/// version "0.9.0" do not count. Guards menu and notifications, which every program of the session can read.
pub fn contains_host_or_address(text: &str) -> bool {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '.' | ':' | '-')))
        .map(|token| token.trim_matches(|c| matches!(c, '.' | ':' | '-')))
        .any(|token| is_host(token) || is_ipv4(token) || is_ipv6(token))
}

fn is_host(token: &str) -> bool {
    let labels: Vec<&str> = token.split('.').collect();
    let Some(last) = labels.last() else { return false };
    labels.len() >= 2
        && labels
            .iter()
            .all(|label| !label.is_empty() && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
        && last.len() >= 2
        && last.chars().all(|c| c.is_ascii_alphabetic())
}

fn is_ipv4(token: &str) -> bool {
    let parts: Vec<&str> = token.split('.').collect();
    parts.len() == 4
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
}

fn is_ipv6(token: &str) -> bool {
    token.matches(':').count() >= 2
        && token.chars().all(|c| c.is_ascii_hexdigit() || c == ':' || c == '.')
        && token.chars().any(|c| c.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::{Error, Observation, ServiceState};
    use crate::servers::{self, parse_list};
    use crate::state::Monitor;

    const FIXTURE: &str = include_str!("../tests/fixtures/servers.json");

    fn fixture() -> Vec<Country> {
        parse_list(FIXTURE.as_bytes()).unwrap()
    }

    // --- Tone ---

    #[test]
    fn punktfarbe_je_zustand() {
        let cases = [
            (DisplayState::NotConnected, Tone::Idle),
            (DisplayState::Connecting, Tone::Busy),
            (DisplayState::Disconnecting, Tone::Busy),
            (
                DisplayState::Switching {
                    target: "de.hideservers.net".to_owned(),
                    back: false,
                },
                Tone::Busy,
            ),
            (
                DisplayState::Switching {
                    target: "nl.hideservers.net".to_owned(),
                    back: true,
                },
                Tone::Busy,
            ),
            (DisplayState::Reapplying { back: false }, Tone::Busy),
            (DisplayState::Reapplying { back: true }, Tone::Busy),
            (
                DisplayState::Connected {
                    server: Some("nl.hideservers.net".to_owned()),
                },
                Tone::Ok,
            ),
            (DisplayState::Connected { server: None }, Tone::Ok),
            (DisplayState::ConnectingBlocked, Tone::Alert),
            (DisplayState::Blocked, Tone::Alert),
            (DisplayState::ServiceUnreachableBlocked, Tone::Alert),
            (DisplayState::ServiceUnreachable, Tone::Alert),
            (DisplayState::NoPermission, Tone::Alert),
        ];
        for (display, expected) in cases {
            assert_eq!(tone(Some(&display)), expected, "{display:?}");
        }
    }

    #[test]
    fn punktfarbe_vor_der_ersten_beobachtung() {
        assert_eq!(tone(None), Tone::Idle);
    }

    #[test]
    fn punktfarbe_als_text() {
        assert_eq!(Tone::Ok.as_str(), "ok");
        assert_eq!(Tone::Busy.as_str(), "busy");
        assert_eq!(Tone::Alert.as_str(), "alert");
        assert_eq!(Tone::Idle.as_str(), "idle");
    }

    // --- StateColor ---

    #[test]
    fn zustandsfarbe_je_zustand() {
        let cases = [
            (DisplayState::NotConnected, StateColor::Normal),
            (DisplayState::Connecting, StateColor::Normal),
            (DisplayState::Disconnecting, StateColor::Normal),
            (
                DisplayState::Switching {
                    target: "de.hideservers.net".to_owned(),
                    back: false,
                },
                StateColor::Normal,
            ),
            (
                DisplayState::Switching {
                    target: "nl.hideservers.net".to_owned(),
                    back: true,
                },
                StateColor::Normal,
            ),
            (DisplayState::Reapplying { back: false }, StateColor::Normal),
            (DisplayState::Reapplying { back: true }, StateColor::Normal),
            (
                DisplayState::Connected {
                    server: Some("nl.hideservers.net".to_owned()),
                },
                StateColor::Connected,
            ),
            (DisplayState::Connected { server: None }, StateColor::Connected),
            (DisplayState::ConnectingBlocked, StateColor::Blocked),
            (DisplayState::Blocked, StateColor::Blocked),
            (DisplayState::ServiceUnreachableBlocked, StateColor::Blocked),
            (DisplayState::ServiceUnreachable, StateColor::Normal),
            (DisplayState::NoPermission, StateColor::Normal),
        ];
        for (display, expected) in cases {
            assert_eq!(state_color(Some(&display)), expected, "{display:?}");
        }
    }

    #[test]
    fn zustandsfarbe_vor_der_ersten_beobachtung() {
        assert_eq!(state_color(None), StateColor::Normal);
    }

    #[test]
    fn zustandsfarbe_im_ablauf_ueber_die_sperre() {
        let color = |monitor: &Monitor| state_color(monitor.display());
        // Internet blocked
        let mut monitor = Monitor::new();
        state(&mut monitor, "routed", None, true);
        assert_eq!(monitor.display(), Some(&DisplayState::Blocked));
        assert_eq!(color(&monitor), StateColor::Blocked);
        // Connecting out of the block: the block persists, red stays
        monitor.begin_connect("de.hideservers.net").unwrap();
        assert_eq!(monitor.display(), Some(&DisplayState::ConnectingBlocked));
        assert_eq!(color(&monitor), StateColor::Blocked);
        state(&mut monitor, "connecting", None, true);
        assert_eq!(color(&monitor), StateColor::Blocked);
        // Connected: green
        state(&mut monitor, "connected", Some("de.hideservers.net"), true);
        assert!(matches!(monitor.display(), Some(DisplayState::Connected { .. })));
        assert_eq!(color(&monitor), StateColor::Connected);
        // Block again and unblock: red until the internet is free, then blue
        let mut monitor = Monitor::new();
        state(&mut monitor, "routed", None, true);
        assert!(monitor.ask_unblock() && monitor.confirm_unblock());
        assert_ne!(color(&monitor), StateColor::Connected, "{:?}", monitor.display());
        monitor.finish_unblock(&Ok(()));
        state(&mut monitor, "clean", None, false);
        assert_eq!(monitor.display(), Some(&DisplayState::NotConnected));
        assert_eq!(color(&monitor), StateColor::Normal);
    }

    #[test]
    fn zustandsfarbe_als_text() {
        assert_eq!(StateColor::Normal.as_str(), "normal");
        assert_eq!(StateColor::Connected.as_str(), "connected");
        assert_eq!(StateColor::Blocked.as_str(), "blocked");
    }

    // --- MainButton ---

    fn state(monitor: &mut Monitor, code: &str, host: Option<&str>, blocked: bool) {
        let state = ServiceState {
            code: code.to_owned(),
            host: host.map(str::to_owned),
            ..ServiceState::default()
        };
        monitor.observe(Observation::State { state, blocked });
    }

    fn button(monitor: &Monitor) -> (&'static str, MainAction) {
        let result = main_button(monitor.display(), &monitor.view());
        (i18n::text(Language::De, result.label), result.action)
    }

    fn connected() -> Monitor {
        let mut monitor = Monitor::new();
        state(&mut monitor, "connected", Some("nl.hideservers.net"), true);
        monitor
    }

    #[test]
    fn hauptknopf_getrennt() {
        let mut monitor = Monitor::new();
        state(&mut monitor, "clean", None, false);
        assert_eq!(button(&monitor), ("Verbinde VPN", MainAction::Connect));
    }

    #[test]
    fn hauptknopf_beim_verbinden() {
        let mut monitor = Monitor::new();
        state(&mut monitor, "clean", None, false);
        monitor.begin_connect("de.hideservers.net").unwrap();
        assert_eq!(button(&monitor), ("Wird verbunden …", MainAction::None));
    }

    #[test]
    fn hauptknopf_verbunden() {
        assert_eq!(button(&connected()), ("Trennen", MainAction::Disconnect));
    }

    #[test]
    fn hauptknopf_beim_wechsel_gesperrt_obwohl_trennen_moeglich() {
        let mut monitor = connected();
        monitor.begin_connect("de.hideservers.net").unwrap();
        assert!(monitor.view().can_disconnect);
        assert_eq!(button(&monitor), ("Wird gewechselt …", MainAction::None));
    }

    #[test]
    fn hauptknopf_beim_trennen() {
        let mut monitor = connected();
        monitor.begin_disconnect(false);
        assert_eq!(monitor.display(), Some(&DisplayState::Disconnecting));
        assert_eq!(button(&monitor), ("Wird getrennt …", MainAction::None));
    }

    #[test]
    fn hauptknopf_bei_gesperrtem_internet() {
        let mut monitor = Monitor::new();
        state(&mut monitor, "routed", None, true);
        assert_eq!(monitor.display(), Some(&DisplayState::Blocked));
        assert_eq!(button(&monitor), ("Verbinde VPN", MainAction::Connect));
    }

    #[test]
    fn hauptknopf_ohne_dienst() {
        let mut monitor = Monitor::new();
        monitor.observe(Observation::Failed {
            error: Error::NotRunning,
            blocked: false,
        });
        assert_eq!(monitor.display(), Some(&DisplayState::ServiceUnreachable));
        // Connecting starts the service first.
        assert_eq!(button(&monitor), ("Verbinde VPN", MainAction::Connect));
        // Without a possible start the button stays disabled, as before.
        monitor.without_start();
        assert_eq!(button(&monitor), ("Verbinde VPN", MainAction::None));
    }

    #[test]
    fn hauptknopf_ohne_berechtigung() {
        let mut monitor = Monitor::new();
        monitor.observe(Observation::Failed {
            error: Error::PermissionDenied,
            blocked: false,
        });
        assert_eq!(monitor.display(), Some(&DisplayState::NoPermission));
        assert_eq!(button(&monitor), ("Verbinde VPN", MainAction::None));
    }

    #[test]
    fn hauptknopf_vor_der_ersten_beobachtung() {
        assert_eq!(button(&Monitor::new()), ("Verbinde VPN", MainAction::None));
    }

    #[test]
    fn wirkung_als_text() {
        assert_eq!(MainAction::Connect.as_str(), "connect");
        assert_eq!(MainAction::Disconnect.as_str(), "disconnect");
        assert_eq!(MainAction::None.as_str(), "none");
    }

    // --- Right area of the main button ---

    /// Right area; checks in passing: the text is empty exactly when nothing is offered.
    fn side(monitor: &Monitor) -> (SideAction, &'static str) {
        let result = main_button(monitor.display(), &monitor.view());
        assert_eq!(
            result.side_label.is_none(),
            result.side == SideAction::None,
            "{result:?}"
        );
        (
            result.side,
            result.side_label.map_or("", |label| i18n::text(Language::De, label)),
        )
    }

    #[test]
    fn rechter_bereich_bietet_sperren_bei_nicht_verbunden() {
        let mut monitor = Monitor::new();
        state(&mut monitor, "clean", None, false);
        assert_eq!(side(&monitor), (SideAction::Route, "Internet sperren"));
        assert_eq!(button(&monitor), ("Verbinde VPN", MainAction::Connect));
    }

    #[test]
    fn rechter_bereich_fehlt_ohne_zustand_bei_verbindung_und_ohne_dienst() {
        let nothing = (SideAction::None, "");
        assert_eq!(side(&Monitor::new()), nothing);
        assert_eq!(side(&connected()), nothing);
        for (error, blocked) in [
            (Error::NotRunning, false),
            (Error::NotRunning, true),
            (Error::PermissionDenied, false),
        ] {
            let mut monitor = Monitor::new();
            monitor.observe(Observation::Failed { error, blocked });
            assert_eq!(side(&monitor), nothing, "{:?}", monitor.display());
        }
    }

    #[test]
    fn rechter_bereich_fehlt_waehrend_eines_vorgangs_ohne_sperre() {
        let nothing = (SideAction::None, "");
        // Connecting from "Not connected"
        let mut connecting = Monitor::new();
        state(&mut connecting, "clean", None, false);
        connecting.begin_connect("de.hideservers.net").unwrap();
        assert_eq!(side(&connecting), nothing);
        // Switching
        let mut switching = connected();
        switching.begin_connect("de.hideservers.net").unwrap();
        assert!(switching.view().switching);
        assert_eq!(side(&switching), nothing);
        // Disconnecting
        let mut disconnecting = connected();
        disconnecting.begin_disconnect(false);
        assert_eq!(side(&disconnecting), nothing);
        // Applying settings
        let mut reapplying = connected();
        reapplying.begin_reapply().unwrap();
        assert!(matches!(reapplying.display(), Some(DisplayState::Reapplying { .. })));
        assert_eq!(side(&reapplying), nothing);
    }

    #[test]
    fn hauptknopf_waehrend_des_sperrens() {
        let mut monitor = Monitor::new();
        state(&mut monitor, "clean", None, false);
        assert!(monitor.begin_route(true));
        assert_eq!(button(&monitor), ("Internet wird gesperrt …", MainAction::None));
        assert_eq!(side(&monitor), (SideAction::None, ""));
    }

    #[test]
    fn rechter_bereich_bietet_aufheben_bei_gesperrtem_internet() {
        let mut monitor = Monitor::new();
        state(&mut monitor, "routed", None, true);
        assert_eq!(monitor.display(), Some(&DisplayState::Blocked));
        assert_eq!(button(&monitor), ("Verbinde VPN", MainAction::Connect));
        assert_eq!(side(&monitor), (SideAction::Unblock, "Sperre aufheben"));
    }

    #[test]
    fn rechter_bereich_bietet_aufheben_wenn_der_dienst_selbst_neu_verbindet() {
        let mut monitor = Monitor::new();
        state(&mut monitor, "connecting", None, true);
        assert_eq!(monitor.display(), Some(&DisplayState::ConnectingBlocked));
        // The main part is not clickable, the way out remains anyway.
        assert_eq!(button(&monitor), ("Verbinde VPN", MainAction::None));
        assert_eq!(side(&monitor), (SideAction::Unblock, "Sperre aufheben"));
    }

    #[test]
    fn rechter_bereich_beim_eigenen_aufbau_aus_der_sperre() {
        let mut monitor = Monitor::new();
        state(&mut monitor, "routed", None, true);
        monitor.begin_connect("de.hideservers.net").unwrap();
        assert_eq!(button(&monitor), ("Wird verbunden …", MainAction::None));
        // The service reports the connection attempt; the block is still in place.
        state(&mut monitor, "connecting", None, true);
        assert!(
            monitor.view().can_unblock,
            "auch während des eigenen Aufbaus gibt es einen Ausweg"
        );
        assert_eq!(button(&monitor), ("Wird verbunden …", MainAction::None));
        assert_eq!(side(&monitor), (SideAction::Unblock, "Sperre aufheben"));
    }

    #[test]
    fn rechter_bereich_fehlt_bei_sperre_ohne_dienst() {
        let mut monitor = Monitor::new();
        monitor.observe(Observation::Failed {
            error: Error::NotRunning,
            blocked: true,
        });
        assert_eq!(monitor.display(), Some(&DisplayState::ServiceUnreachableBlocked));
        assert_eq!(side(&monitor), (SideAction::None, ""));
    }

    #[test]
    fn aufheben_hat_vorrang_und_folgt_genau_can_unblock() {
        let mut cases = Vec::new();
        for (code, blocked) in [
            ("clean", false),
            ("routed", true),
            ("dpd timeout", true),
            ("connecting", true),
            ("connecting", false),
            ("connected", true),
        ] {
            let mut monitor = Monitor::new();
            state(&mut monitor, code, Some("nl.hideservers.net"), blocked);
            cases.push(monitor);
        }
        let mut own = Monitor::new();
        state(&mut own, "routed", None, true);
        own.begin_connect("de.hideservers.net").unwrap();
        cases.push(own);
        let mut lifting = Monitor::new();
        state(&mut lifting, "routed", None, true);
        assert!(lifting.ask_unblock() && lifting.confirm_unblock());
        cases.push(lifting);
        for monitor in &cases {
            let (action, _) = side(monitor);
            assert_eq!(
                action == SideAction::Unblock,
                monitor.view().can_unblock,
                "{:?}",
                monitor.display()
            );
            if monitor.view().can_unblock {
                assert_ne!(action, SideAction::Route, "{:?}", monitor.display());
            }
        }
    }

    #[test]
    fn rechter_bereich_als_text() {
        assert_eq!(SideAction::Route.as_str(), "route");
        assert_eq!(SideAction::Unblock.as_str(), "unblock");
        assert_eq!(SideAction::None.as_str(), "none");
    }

    // --- location_of and OverviewLocation ---

    #[test]
    fn standort_eines_bekannten_servers() {
        let countries = fixture();
        assert_eq!(
            location_of("frankfurt.hideservers.net", &countries, Language::De),
            Some(("DE".to_owned(), "Deutschland, Frankfurt".to_owned()))
        );
    }

    #[test]
    fn standort_eines_streaming_servers() {
        assert_eq!(
            location_of("streaming-uk.hideservers.net", &fixture(), Language::De),
            Some(("GB".to_owned(), "Streaming, Vereinigtes Königreich".to_owned()))
        );
    }

    #[test]
    fn standort_eines_unbekannten_servers() {
        assert_eq!(location_of("unbekannt.hideservers.net", &fixture(), Language::De), None);
        assert_eq!(location_of("frankfurt.hideservers.net", &[], Language::De), None);
    }

    fn place(code: Option<&str>, label: &str) -> OverviewLocation {
        OverviewLocation {
            code: code.map(str::to_owned),
            label: label.to_owned(),
            best: false,
        }
    }

    #[test]
    fn uebersicht_zeigt_den_verbundenen_bekannten_server() {
        let location = overview_location("frankfurt.hideservers.net", None, "", "", &fixture(), Language::De);
        assert_eq!(location, place(Some("DE"), "Deutschland, Frankfurt"));
    }

    #[test]
    fn uebersicht_zeigt_den_verbundenen_unbekannten_server() {
        let location = overview_location("unbekannt.hideservers.net", None, "", "", &fixture(), Language::De);
        assert_eq!(location, place(None, "unbekannt.hideservers.net"));
    }

    #[test]
    fn uebersicht_zeigt_den_gewaehlten_server() {
        let countries = fixture();
        let location = overview_location(
            "",
            None,
            "au.hideservers.net",
            "Australien, Automatisch",
            &countries,
            Language::De,
        );
        assert_eq!(location, place(Some("AU"), "Australien, Automatisch"));
        let unknown = overview_location(
            "",
            None,
            "unbekannt.hideservers.net",
            "Irgendwo",
            &countries,
            Language::De,
        );
        assert_eq!(unknown, place(None, "Irgendwo"));
    }

    /// Feature 013: without a selection, or with the selection "best", the overview shows "Best location".
    #[test]
    fn uebersicht_ohne_wahl_zeigt_den_besten_standort() {
        for selected in ["", "best"] {
            for (language, name) in [(Language::De, "Bester Standort"), (Language::En, "Best location")] {
                let location = overview_location("", None, selected, "", &fixture(), language);
                assert_eq!(
                    location,
                    OverviewLocation {
                        code: None,
                        label: name.to_owned(),
                        best: true
                    },
                    "{selected:?}"
                );
                // Also while the list is loading (no countries yet).
                assert_eq!(overview_location("", None, selected, "", &[], language), location);
            }
        }
    }

    #[test]
    fn uebersicht_vorrang_verbunden_ziel_auswahl() {
        let countries = fixture();
        // Connecting through "Best location": the target is shown.
        let target = overview_location("", Some("berlin.hideservers.net"), "best", "", &countries, Language::De);
        assert_eq!(target, place(Some("DE"), "Deutschland, Berlin"));
        // Connected wins over the target and over the selection.
        let connected = overview_location(
            "nl.hideservers.net",
            Some("berlin.hideservers.net"),
            "best",
            "",
            &countries,
            Language::De,
        );
        assert_eq!(connected.code.as_deref(), Some("NL"));
        assert!(!connected.best);
        // After disconnecting the selection "best" applies again, an own selection stays.
        assert!(overview_location("", None, "best", "", &countries, Language::De).best);
        assert_eq!(
            overview_location(
                "",
                None,
                "berlin.hideservers.net",
                "Deutschland, Berlin",
                &countries,
                Language::De
            ),
            place(Some("DE"), "Deutschland, Berlin")
        );
        // An empty target counts as none.
        assert!(overview_location("", Some(""), "best", "", &countries, Language::De).best);
    }

    #[test]
    fn verbundener_server_hat_vorrang_vor_dem_gewaehlten() {
        let location = overview_location(
            "streaming-uk.hideservers.net",
            None,
            "au.hideservers.net",
            "Australien, Automatisch",
            &fixture(),
            Language::De,
        );
        assert_eq!(location.code.as_deref(), Some("GB"));
        assert_eq!(location.label, "Streaming, Vereinigtes Königreich");
    }

    #[test]
    fn ziel_mit_ort_und_hostname() {
        let countries = fixture();
        assert_eq!(
            target_label("berlin.hideservers.net", &countries, Language::De),
            "Deutschland, Berlin (berlin.hideservers.net)"
        );
        assert_eq!(
            target_short("berlin.hideservers.net", &countries, Language::En),
            "Germany, Berlin"
        );
        assert_eq!(
            target_label("streaming-jp.hideservers.net", &countries, Language::De),
            "Streaming Japan (streaming-jp.hideservers.net)"
        );
        assert_eq!(
            target_short("streaming-jp.hideservers.net", &countries, Language::De),
            "Streaming Japan"
        );
        // Unknown host: its name, in both places.
        assert_eq!(
            target_label("x.hideservers.net", &countries, Language::De),
            "x.hideservers.net"
        );
        assert_eq!(
            target_short("x.hideservers.net", &[], Language::De),
            "x.hideservers.net"
        );
    }

    #[test]
    fn dauer_und_datenmenge_auf_englisch() {
        assert_eq!(duration_text(Duration::from_secs(59), Language::En), "less than 1 min");
        assert_eq!(duration_text(Duration::from_secs(180), Language::En), "3 min");
        assert_eq!(duration_text(Duration::from_mins(64), Language::En), "1 h 4 min");
        assert_eq!(bytes_text(999, Language::En), "999 B");
        assert_eq!(bytes_text(1500, Language::En), "1.5 kB");
        assert_eq!(bytes_text(12_345_678, Language::En), "12.3 MB");
        assert_eq!(bytes_text(2_000_000_000, Language::En), "2.0 GB");
    }

    #[test]
    fn hauptknopf_auf_englisch() {
        let mut monitor = Monitor::new();
        state(&mut monitor, "clean", None, false);
        let result = main_button(monitor.display(), &monitor.view());
        assert_eq!(i18n::text(Language::En, result.label), "Connect VPN");
        assert_eq!(
            result.side_label.map(|label| i18n::text(Language::En, label)),
            Some("Block internet")
        );
    }

    // --- Location list (feature 012) ---

    const LIST: &[u8] = include_bytes!("../tests/fixtures/servers-2026-11-19.json");

    fn real() -> Vec<Country> {
        parse_list(LIST).unwrap()
    }

    static NONE: BTreeSet<String> = BTreeSet::new();

    fn ctx(countries: &[Country], language: Language) -> ListContext<'_> {
        ListContext {
            countries,
            language,
            query: "",
            free_only: false,
            expanded: &NONE,
            selected: "",
            connected: None,
            favorites: &[],
            loading: false,
            best_host: None,
        }
    }

    fn kinds(rows: &[LocationItem]) -> Vec<ItemKind> {
        rows.iter().map(|row| row.kind).collect()
    }

    fn country_row<'a>(rows: &'a [LocationItem], code: &str) -> &'a LocationItem {
        rows.iter()
            .find(|row| row.code == code && matches!(row.kind, ItemKind::Country | ItemKind::Location))
            .unwrap()
    }

    fn below<'a>(rows: &'a [LocationItem], code: &str) -> Vec<&'a LocationItem> {
        rows.iter()
            .filter(|row| row.code == code && row.kind == ItemKind::Child)
            .collect()
    }

    #[test]
    fn zugeklappt_eine_zeile_je_land() {
        let countries = real();
        for language in Language::ALL {
            let rows = location_items(&ctx(&countries, language));
            assert_eq!(rows[0].kind, ItemKind::Heading);
            let lands = rows
                .iter()
                .filter(|row| matches!(row.kind, ItemKind::Location | ItemKind::Country))
                .count();
            assert_eq!(lands, 57, "eine Zeile je Land");
            // Heading, "Best location" (feature 013), 57 countries.
            assert_eq!(rows[1].kind, ItemKind::Best);
            assert_eq!(rows.len(), 59);
            assert_eq!(rows.iter().filter(|row| row.kind == ItemKind::Country).count(), 11);
        }
        let rows = location_items(&ctx(&countries, Language::En));
        assert_eq!(rows[0].country, "All locations");
        let rows = location_items(&ctx(&countries, Language::De));
        assert_eq!(rows[0].country, "Alle Standorte");
    }

    #[test]
    fn land_mit_einem_ort_wie_bisher() {
        let countries = real();
        let rows = location_items(&ctx(&countries, Language::De));
        let austria = country_row(&rows, "AT");
        assert_eq!(austria.kind, ItemKind::Location);
        assert_eq!(austria.country, "Österreich");
        assert_eq!(austria.place, "Wien");
        assert_eq!(austria.choice, "Österreich, Wien");
        assert_eq!(austria.star_host, austria.host);
        assert!(!austria.expandable && !austria.favorite);
    }

    #[test]
    fn land_mit_mehreren_orten_steht_fuer_automatisch() {
        let countries = real();
        let rows = location_items(&ctx(&countries, Language::De));
        let germany = country_row(&rows, "DE");
        assert_eq!(germany.kind, ItemKind::Country);
        assert_eq!(germany.country, "Deutschland");
        assert_eq!(germany.host, "de.hideservers.net");
        assert_eq!(germany.star_host, "de.hideservers.net");
        assert_eq!(germany.place, "Automatisch, hide.me wählt den Ort");
        assert_eq!(germany.choice, "Deutschland, Automatisch");
        assert!(germany.expandable && !germany.expanded);
        assert!(below(&rows, "DE").is_empty());
        let finland = country_row(&rows, "FI");
        assert_eq!(finland.host, "fi.hideservers.net");
        assert_eq!(finland.choice, "Finnland, Automatisch");
        let english = location_items(&ctx(&countries, Language::En));
        assert_eq!(
            country_row(&english, "DE").place,
            "Automatic, hide.me chooses the location"
        );
    }

    #[test]
    fn aufgeklappt_folgen_die_orte_ohne_laendernamen() {
        let countries = real();
        let expanded = BTreeSet::from(["DE".to_owned()]);
        let rows = location_items(&ListContext {
            expanded: &expanded,
            ..ctx(&countries, Language::De)
        });
        let germany = country_row(&rows, "DE");
        assert!(germany.expanded);
        let children = below(&rows, "DE");
        let places: Vec<&str> = children.iter().map(|row| row.place.as_str()).collect();
        assert_eq!(places, ["Berlin", "Frankfurt #1", "Frankfurt #2", "Frankfurt"]);
        assert!(children.iter().all(|row| row.country.is_empty() && row.code == "DE"));
        assert_eq!(children[0].choice, "Deutschland, Berlin");
        assert_eq!(children[0].star_host, children[0].host);
        assert!(children[3].free);
        // Directly below the country row.
        let index = rows.iter().position(|row| row.code == "DE").unwrap();
        assert_eq!(rows[index + 1].kind, ItemKind::Child);
        let english = location_items(&ListContext {
            expanded: &expanded,
            ..ctx(&countries, Language::En)
        });
        assert_eq!(below(&english, "DE")[0].choice, "Germany, Berlin");
    }

    #[test]
    fn gescheiterter_gebildeter_name_klappt_nur_noch_auf() {
        let mut countries = real();
        servers::drop_derived(&mut countries, &BTreeSet::from(["fi.hideservers.net".to_owned()]));
        let rows = location_items(&ctx(&countries, Language::De));
        let finland = country_row(&rows, "FI");
        assert_eq!(finland.kind, ItemKind::Country);
        assert!(finland.host.is_empty() && finland.star_host.is_empty() && finland.place.is_empty());
        assert!(finland.expandable);
        let expanded = BTreeSet::from(["FI".to_owned()]);
        let rows = location_items(&ListContext {
            expanded: &expanded,
            ..ctx(&countries, Language::De)
        });
        assert_eq!(below(&rows, "FI").len(), 3);
    }

    #[test]
    fn nur_kostenlose_ohne_pfeil() {
        let countries = real();
        let rows = location_items(&ListContext {
            free_only: true,
            ..ctx(&countries, Language::De)
        });
        let germany = country_row(&rows, "DE");
        assert_eq!(germany.kind, ItemKind::Location);
        assert_eq!(germany.place, "Frankfurt");
        assert!(germany.free && !germany.expandable);
        // "Free only" never hides "Best location" (feature 013).
        assert_eq!(rows[1].kind, ItemKind::Best);
        assert!(rows.iter().skip(2).all(|row| row.free), "nur kostenlose");
    }

    #[test]
    fn suche_klappt_auf_wenn_nur_orte_passen() {
        let countries = real();
        let expanded = BTreeSet::new();
        let search = |query| {
            location_items(&ListContext {
                query,
                expanded: &expanded,
                ..ctx(&countries, Language::De)
            })
        };
        let rows = search("frank");
        let germany = country_row(&rows, "DE");
        assert!(germany.expanded);
        let places: Vec<&str> = below(&rows, "DE").iter().map(|row| row.place.as_str()).collect();
        assert_eq!(places, ["Frankfurt #1", "Frankfurt #2", "Frankfurt"]);
        // "Frankreich" matches by its name: folded.
        assert!(!country_row(&rows, "FR").expanded);
        let rows = search("deutsch");
        assert!(!country_row(&rows, "DE").expanded);
        assert!(below(&rows, "DE").is_empty());
        assert!(expanded.is_empty(), "die Suche ändert den Klappzustand nicht");
        assert!(search("Atlantis").is_empty());
    }

    #[test]
    fn auswahl_und_verbindung_im_zugeklappten_land() {
        let countries = real();
        let berlin = "berlin.hideservers.net";
        let folded = location_items(&ListContext {
            selected: berlin,
            connected: Some(berlin),
            ..ctx(&countries, Language::De)
        });
        let germany = country_row(&folded, "DE");
        assert!(germany.holds_selected && germany.holds_connected);
        let expanded = BTreeSet::from(["DE".to_owned()]);
        let open = location_items(&ListContext {
            selected: berlin,
            connected: Some(berlin),
            expanded: &expanded,
            ..ctx(&countries, Language::De)
        });
        let germany = country_row(&open, "DE");
        assert!(!germany.holds_selected && !germany.holds_connected);
        // The country row itself is "Automatic": selected through its own host, not through `holds_selected`.
        let automatic = location_items(&ListContext {
            selected: "de.hideservers.net",
            ..ctx(&countries, Language::De)
        });
        assert!(!country_row(&automatic, "DE").holds_selected);
    }

    #[test]
    fn sortiert_nach_der_sprache() {
        let countries = real();
        let order = |language| -> Vec<String> {
            location_items(&ctx(&countries, language))
                .into_iter()
                .filter(|row| row.kind != ItemKind::Heading)
                .map(|row| row.country)
                .collect()
        };
        let english = order(Language::En);
        let position = |list: &[String], name: &str| list.iter().position(|known| known == name).unwrap();
        assert!(position(&english, "Germany") < position(&english, "Hong Kong"));
        let german = order(Language::De);
        assert!(position(&german, "Deutschland") < position(&german, "Finnland"));
    }

    // --- "Best location" (feature 013) ---

    fn best_row(rows: &[LocationItem]) -> Vec<&LocationItem> {
        rows.iter().filter(|row| row.kind == ItemKind::Best).collect()
    }

    #[test]
    fn bester_standort_steht_zuerst_unter_alle_standorte() {
        let countries = real();
        for (language, name, hint) in [
            (
                Language::De,
                "Bester Standort",
                "Automatisch, Celata wählt den Standort",
            ),
            (Language::En, "Best location", "Automatic, Celata chooses the location"),
        ] {
            let rows = location_items(&ctx(&countries, language));
            assert_eq!(rows[0].kind, ItemKind::Heading);
            let row = &rows[1];
            assert_eq!(row.kind, ItemKind::Best);
            assert_eq!(row.kind.as_str(), "best");
            assert_eq!(row.host, "best");
            assert_eq!(row.star_host, "best");
            assert_eq!(row.code, "");
            assert_eq!(row.country, name);
            assert_eq!(row.place, hint);
            assert_eq!(row.choice, name);
            assert!(!row.favorite && !row.free && !row.expandable && !row.holds_connected && !row.holds_selected);
            assert_eq!(best_row(&rows).len(), 1);
        }
        // Without countries (loading or failed) there is no row below "All locations".
        for loading in [true, false] {
            let rows = location_items(&ListContext {
                loading,
                ..ctx(&[], Language::De)
            });
            assert!(best_row(&rows).is_empty(), "{loading}");
            assert_eq!(rows.len(), 1);
        }
        // Never in the tab "Streaming".
        let streaming = streaming_items(&streaming::all(), &ctx(&countries, Language::De));
        assert!(best_row(&streaming).is_empty());
        assert!(streaming.iter().all(|row| row.host != "best"));
    }

    #[test]
    fn suche_nach_dem_besten_standort() {
        let countries = real();
        let search = |query: &'static str| {
            location_items(&ListContext {
                query,
                ..ctx(&countries, Language::De)
            })
        };
        for query in ["best", "bester", "BEST", " Standort "] {
            assert_eq!(best_row(&search(query)).len(), 1, "{query}");
        }
        let berlin = search("berlin");
        assert!(best_row(&berlin).is_empty());
        // Germany unfolded with Berlin.
        assert!(
            berlin
                .iter()
                .any(|row| row.kind == ItemKind::Child && row.host == "berlin.hideservers.net")
        );
        // A search with only "Best location" as hit keeps the heading.
        let rows = search("bester");
        assert_eq!(rows[0].kind, ItemKind::Heading);
        assert_eq!(rows[1].kind, ItemKind::Best);
    }

    #[test]
    fn gewaehlt_nur_bei_verbindung_mit_dem_ermittelten_server() {
        let countries = real();
        let connected = ListContext {
            connected: Some("berlin.hideservers.net"),
            best_host: Some("berlin.hideservers.net"),
            ..ctx(&countries, Language::De)
        };
        let rows = location_items(&connected);
        let row = best_row(&rows)[0];
        assert_eq!(row.place, "Gewählt: Deutschland, Berlin");
        assert_eq!(row.country, "Bester Standort");
        // The country row carries "Connected", not "Best location" (it has no such mark).
        let germany = country_row(&rows, "DE");
        assert!(germany.holds_connected);
        assert!(!row.holds_connected);
        let english = location_items(&ListContext {
            language: Language::En,
            ..connected
        });
        assert_eq!(best_row(&english)[0].place, "Chosen: Germany, Berlin");
        // Connected elsewhere, not connected, or chosen by the user: the hint.
        for (connected, best_host) in [
            (Some("nl.hideservers.net"), Some("berlin.hideservers.net")),
            (None, Some("berlin.hideservers.net")),
            (Some("berlin.hideservers.net"), None),
        ] {
            let rows = location_items(&ListContext {
                connected,
                best_host,
                ..ctx(&countries, Language::De)
            });
            assert_eq!(best_row(&rows)[0].place, "Automatisch, Celata wählt den Standort");
        }
    }

    #[test]
    fn bester_standort_als_favorit() {
        let countries = real();
        let favorites = [
            "frankfurt1.hideservers.net".to_owned(),
            "best".to_owned(),
            "nl.hideservers.net".to_owned(),
        ];
        let rows = location_items(&ListContext {
            favorites: &favorites,
            ..ctx(&countries, Language::De)
        });
        assert_eq!(rows[0].country, "Favoriten");
        let hosts: Vec<&str> = rows[1..4].iter().map(|row| row.host.as_str()).collect();
        assert_eq!(hosts, ["frankfurt1.hideservers.net", "best", "nl.hideservers.net"]);
        let both = best_row(&rows);
        assert_eq!(both.len(), 2);
        assert!(both.iter().all(|row| row.favorite && row.star_host == "best"));
        // Also below "All locations" as first row.
        let all = rows.iter().position(|row| row.country == "Alle Standorte").unwrap();
        assert_eq!(rows[all + 1].kind, ItemKind::Best);
        // The search applies to the favourite as to the row.
        let berlin = location_items(&ListContext {
            favorites: &favorites,
            query: "berlin",
            ..ctx(&countries, Language::De)
        });
        assert!(best_row(&berlin).is_empty());
        // List not loadable: no row below "All locations", but the favourite stays.
        let failed = location_items(&ListContext {
            favorites: &favorites,
            ..ctx(&[], Language::De)
        });
        let rows = best_row(&failed);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, ItemKind::Best);
        assert_eq!(failed[0].country, "Favoriten");
        // While loading without countries: loading like the other favourites, with its name.
        let loading = location_items(&ListContext {
            favorites: &favorites,
            loading: true,
            ..ctx(&[], Language::De)
        });
        let row = loading.iter().find(|row| row.star_host == "best").unwrap();
        assert_eq!(row.kind, ItemKind::Loading);
        assert_eq!(row.place, "Bester Standort");
        assert_eq!(row.host, "");
    }

    #[test]
    fn bester_standort_in_den_lieblingsstandorten() {
        let countries = real();
        let favorites = ["best".to_owned(), "nl.hideservers.net".to_owned()];
        let rows = favorite_items(
            &favorites,
            &countries,
            &[],
            false,
            Some("berlin.hideservers.net"),
            Language::De,
        );
        let row = &rows[0];
        assert!(row.best);
        assert_eq!(row.label, "Bester Standort");
        assert_eq!(row.choice, "Bester Standort");
        assert_eq!(row.host, "best");
        assert_eq!(row.star_host, "best");
        assert_eq!(row.code, "");
        assert_eq!(row.state, favorites::State::Available);
        assert!(!row.connected && !row.streaming);
        assert!(!rows[1].best);
        let english = favorite_items(&favorites, &countries, &[], false, None, Language::En);
        assert_eq!(english[0].label, "Best location");
        // Never missing; while loading not selectable.
        let failed = favorite_items(&favorites, &[], &[], false, None, Language::De);
        assert_eq!(failed[0].state, favorites::State::Available);
        assert_eq!(failed[0].host, "best");
        let loading = favorite_items(&favorites, &[], &[], true, None, Language::De);
        assert_eq!(loading[0].state, favorites::State::Loading);
        assert_eq!(loading[0].host, "");
        assert_eq!(loading[0].label, "Bester Standort");
    }

    #[test]
    fn bester_standort_aendert_bereichsersatz_und_scrollen_nicht() {
        // Rows of "Best location" go through `splice` like every row: only the changed row is replaced.
        let countries = real();
        let before = location_items(&ctx(&countries, Language::De));
        let after = location_items(&ListContext {
            connected: Some("berlin.hideservers.net"),
            best_host: Some("berlin.hideservers.net"),
            ..ctx(&countries, Language::De)
        });
        assert_eq!(before.len(), after.len());
        let (start, removed, inserted) = splice(&before, &after).unwrap();
        // The change starts at "Best location" and ends at Germany ("Connected"); no row more or less.
        assert_eq!(start, 1);
        assert_eq!(removed, inserted);
        // Same number of rows: the list does not move.
        assert!((scroll_after(Some((start, removed, inserted)), -520.0) + 520.0).abs() < f32::EPSILON);
    }

    #[test]
    fn kleine_liste_wie_bisher() {
        let rows = location_items(&ctx(&fixture(), Language::De));
        let names: Vec<&str> = rows.iter().skip(1).map(|row| row.country.as_str()).collect();
        assert_eq!(
            names,
            [
                "Bester Standort",
                "Australien",
                "Deutschland",
                "Neuseeland",
                "Niederlande"
            ]
        );
        let australia = country_row(&rows, "AU");
        assert_eq!(australia.choice, "Australien, Automatisch");
        let rows = location_items(&ListContext {
            query: "perth",
            ..ctx(&fixture(), Language::De)
        });
        assert_eq!(kinds(&rows), [ItemKind::Heading, ItemKind::Country, ItemKind::Child]);
        assert_eq!(rows[2].choice, "Australien, Perth");
    }

    // --- Favourites in the lists (feature 012) ---

    fn favorites_ctx<'a>(countries: &'a [Country], favorites: &'a [String], language: Language) -> ListContext<'a> {
        ListContext {
            favorites,
            ..ctx(countries, language)
        }
    }

    fn hosts(list: &[&str]) -> Vec<String> {
        list.iter().map(|host| (*host).to_owned()).collect()
    }

    #[test]
    fn favoriten_oben_in_gespeicherter_reihenfolge() {
        let countries = real();
        let favorites = hosts(&[
            "frankfurt1.hideservers.net",
            "de.hideservers.net",
            "streaming-jp.hideservers.net",
        ]);
        let rows = location_items(&favorites_ctx(&countries, &favorites, Language::De));
        assert_eq!(rows[0].kind, ItemKind::Heading);
        assert_eq!(rows[0].country, "Favoriten");
        assert_eq!(
            (rows[1].country.as_str(), rows[1].place.as_str()),
            ("Deutschland", "Frankfurt #1")
        );
        assert_eq!(
            (rows[2].country.as_str(), rows[2].place.as_str()),
            ("Deutschland", "Automatisch")
        );
        assert_eq!(rows[2].choice, "Deutschland, Automatisch");
        for row in &rows[1..3] {
            assert_eq!(row.kind, ItemKind::Location);
            assert!(row.favorite);
            assert_eq!(row.star_host, row.host);
        }
        assert_eq!(rows[3].kind, ItemKind::Heading);
        assert_eq!(rows[3].country, "Alle Standorte");
        // The same location keeps its yellow star under "All locations".
        let germany = country_row(&rows[3..], "DE");
        assert!(germany.favorite);
        let english = location_items(&favorites_ctx(&countries, &favorites, Language::En));
        assert_eq!(english[0].country, "Favorites");
        assert_eq!(english[3].country, "All locations");
        // Streaming favourites only in their own tab.
        assert!(!rows.iter().any(|row| row.host.starts_with("streaming-")));
    }

    #[test]
    fn ohne_favoriten_nur_alle_standorte() {
        let countries = real();
        let rows = location_items(&ctx(&countries, Language::De));
        assert_eq!(rows.iter().filter(|row| row.kind == ItemKind::Heading).count(), 1);
        assert_eq!(rows[0].country, "Alle Standorte");
        let streaming_only = hosts(&["streaming-jp.hideservers.net"]);
        let rows = location_items(&favorites_ctx(&countries, &streaming_only, Language::De));
        assert_eq!(rows[0].country, "Alle Standorte");
    }

    #[test]
    fn suche_wirkt_auf_favoriten_kostenlos_nicht() {
        let countries = real();
        let favorites = hosts(&[
            "frankfurt1.hideservers.net",
            "vienna.hideservers.net",
            "unbekannt-test.hideservers.net",
        ]);
        let rows = location_items(&ListContext {
            query: "frank",
            ..favorites_ctx(&countries, &favorites, Language::De)
        });
        let favorites_part: Vec<&LocationItem> = rows
            .iter()
            .skip(1)
            .take_while(|row| row.kind != ItemKind::Heading)
            .collect();
        assert_eq!(favorites_part.len(), 1);
        assert_eq!(favorites_part[0].host, "frankfurt1.hideservers.net");
        // A favourite that is not found shows only through its host name.
        let rows = location_items(&ListContext {
            query: "unbekannt",
            ..favorites_ctx(&countries, &favorites, Language::De)
        });
        assert_eq!(kinds(&rows), [ItemKind::Heading, ItemKind::Missing]);
        assert!(
            location_items(&ListContext {
                query: "Atlantis",
                ..favorites_ctx(&countries, &favorites, Language::De)
            })
            .is_empty()
        );
        // "Free only" leaves the favourites alone.
        let rows = location_items(&ListContext {
            free_only: true,
            ..favorites_ctx(&countries, &favorites, Language::De)
        });
        assert_eq!(rows[1].host, "frankfurt1.hideservers.net");
        assert!(!rows[1].free);
    }

    #[test]
    fn favorit_im_zugeklappten_land_laesst_es_zu() {
        let countries = real();
        let favorites = hosts(&["berlin.hideservers.net"]);
        let rows = location_items(&favorites_ctx(&countries, &favorites, Language::De));
        assert!(!country_row(&rows[2..], "DE").expanded);
        assert_eq!(rows[1].choice, "Deutschland, Berlin");
    }

    #[test]
    fn fehlender_und_ladender_favorit() {
        let countries = real();
        let favorites = hosts(&["unbekannt-test.hideservers.net", "fi.hideservers.net"]);
        let mut dropped = countries.clone();
        servers::drop_derived(&mut dropped, &BTreeSet::from(["fi.hideservers.net".to_owned()]));
        let rows = location_items(&favorites_ctx(&dropped, &favorites, Language::De));
        for row in &rows[1..3] {
            assert_eq!(row.kind, ItemKind::Missing);
            assert!(row.host.is_empty() && row.code.is_empty());
            assert_eq!(row.place, row.star_host);
            assert!(row.favorite);
        }
        assert_eq!(rows[1].place, "unbekannt-test.hideservers.net");
        let loading = location_items(&ListContext {
            loading: true,
            ..favorites_ctx(&[], &favorites, Language::De)
        });
        assert_eq!(
            kinds(&loading),
            [
                ItemKind::Heading,
                ItemKind::Loading,
                ItemKind::Loading,
                ItemKind::Heading
            ]
        );
        assert_eq!(loading[3].country, "Alle Standorte");
    }

    #[test]
    fn jede_waehlbare_zeile_hat_einen_stern() {
        let countries = real();
        let expanded: BTreeSet<String> = countries.iter().map(|country| country.code.clone()).collect();
        let rows = location_items(&ListContext {
            expanded: &expanded,
            ..ctx(&countries, Language::De)
        });
        for row in &rows {
            assert_eq!(row.star_host, row.host, "{row:?}");
        }
        let streaming = streaming_items(&streaming::all(), &ctx(&countries, Language::De));
        assert!(
            streaming
                .iter()
                .skip(1)
                .all(|row| row.star_host == row.host && !row.host.is_empty())
        );
    }

    #[test]
    fn streaming_reiter_mit_eigenen_favoriten() {
        let favorites = hosts(&["streaming-jp.hideservers.net", "de.hideservers.net"]);
        let all = streaming::all();
        let rows = streaming_items(&all, &favorites_ctx(&[], &favorites, Language::De));
        assert_eq!(rows[0].country, "Favoriten");
        assert_eq!(rows[1].host, "streaming-jp.hideservers.net");
        assert_eq!(rows[1].choice, "Streaming, Japan");
        assert!(rows[1].favorite && rows[1].streaming);
        assert_eq!(rows[2].country, "Alle Standorte");
        assert_eq!(rows.len(), 3 + all.len());
        let uk = rows.iter().find(|row| row.code == "GB").unwrap();
        assert_eq!(uk.country, "Vereinigtes Königreich");
        assert_eq!(uk.place, "Streaming");
        assert_eq!(uk.choice, "Streaming, Vereinigtes Königreich");
        // Hidden after the check: missing among the favourites.
        let hidden = streaming::visible(&streaming::Check::Checked(BTreeSet::from([
            "streaming-jp.hideservers.net".to_owned(),
        ])));
        let rows = streaming_items(&hidden, &favorites_ctx(&[], &favorites, Language::De));
        assert_eq!(rows[1].kind, ItemKind::Missing);
        // Sorted in English.
        let english = streaming_items(&all, &ctx(&[], Language::En));
        let names: Vec<&str> = english.iter().skip(1).map(|row| row.country.as_str()).collect();
        assert!(
            names
                .windows(2)
                .all(|pair| crate::names::sort_key(pair[0]) <= crate::names::sort_key(pair[1]))
        );
        // Search.
        let rows = streaming_items(
            &all,
            &ListContext {
                query: "japan",
                ..ctx(&[], Language::En)
            },
        );
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn zeilenart_als_text() {
        assert_eq!(ItemKind::Heading.as_str(), "heading");
        assert_eq!(ItemKind::Location.as_str(), "location");
        assert_eq!(ItemKind::Country.as_str(), "country");
        assert_eq!(ItemKind::Child.as_str(), "child");
        assert_eq!(ItemKind::Missing.as_str(), "missing");
        assert_eq!(ItemKind::Loading.as_str(), "loading");
    }

    // --- favorite_items (feature 012) ---

    #[test]
    fn lieblingsstandorte_gemischt_in_gespeicherter_reihenfolge() {
        let countries = real();
        let favorites = hosts(&[
            "streaming-jp.hideservers.net",
            "frankfurt1.hideservers.net",
            "unbekannt-test.hideservers.net",
            "de.hideservers.net",
        ]);
        let all = streaming::all();
        let items = favorite_items(
            &favorites,
            &countries,
            &all,
            false,
            Some("frankfurt1.hideservers.net"),
            Language::De,
        );
        let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "Japan",
                "Deutschland, Frankfurt #1",
                "unbekannt-test.hideservers.net",
                "Deutschland, Automatisch"
            ]
        );
        assert!(items[0].streaming && !items[1].streaming);
        assert_eq!(items[0].code, "JP");
        assert!(items[1].connected && !items[0].connected && !items[3].connected);
        assert_eq!(items[2].state, favorites::State::Missing);
        assert!(items[2].host.is_empty() && items[2].code.is_empty());
        assert_eq!(items[2].star_host, "unbekannt-test.hideservers.net");
        assert_eq!(items[0].choice, "Streaming, Japan");
        for item in &items[1..] {
            assert_eq!(item.choice, item.label);
        }
        for item in &items {
            assert_eq!(
                item.star_host,
                favorites[items.iter().position(|known| known == item).unwrap()]
            );
        }
        assert_eq!(items[1].host, "frankfurt1.hideservers.net");
        let english = favorite_items(&favorites, &countries, &all, false, None, Language::En);
        assert_eq!(english[0].label, "Japan");
        assert_eq!(english[0].choice, "Streaming, Japan");
        assert_eq!(english[1].label, "Germany, Frankfurt #1");
        // While loading: standard favourites are loading, streaming ones available.
        let loading = favorite_items(&favorites, &[], &all, true, None, Language::De);
        assert_eq!(loading[0].state, favorites::State::Available);
        assert_eq!(loading[1].state, favorites::State::Loading);
        assert_eq!(loading[1].label, "frankfurt1.hideservers.net");
    }

    // --- "All locations" always ---

    #[test]
    fn alle_standorte_auch_beim_laden_und_ohne_liste() {
        for loading in [true, false] {
            let rows = location_items(&ListContext {
                loading,
                ..ctx(&[], Language::De)
            });
            assert_eq!(kinds(&rows), [ItemKind::Heading], "loading: {loading}");
            assert_eq!(rows[0].country, "Alle Standorte");
        }
        let streaming = streaming_items(&[], &ctx(&[], Language::En));
        assert_eq!(streaming.len(), 1);
        assert_eq!(streaming[0].country, "All locations");
        // Only a search without hits leaves it out.
        let rows = location_items(&ListContext {
            query: "Atlantis",
            ..ctx(&[], Language::De)
        });
        assert!(rows.is_empty());
    }

    // --- scroll_after (feature 012) ---

    #[test]
    fn scrollstand_bleibt_bei_aenderungen_oberhalb() {
        let row = ROW_HEIGHT;
        // Scrolled down by 10 rows; two rows inserted at the top: the list moves down by two rows.
        assert_eq!(scroll_after(Some((1, 0, 2)), -10.0 * row), -12.0 * row);
        // One row removed above: up by one row.
        assert_eq!(scroll_after(Some((1, 1, 0)), -10.0 * row), -9.0 * row);
        // A row changed above (same number): stays.
        assert_eq!(scroll_after(Some((3, 1, 1)), -10.0 * row), -10.0 * row);
        // Change inside the visible area (unfolding a country): stays.
        assert_eq!(scroll_after(Some((12, 0, 4)), -10.0 * row), -10.0 * row);
        // Exactly at the first visible row: counts as visible.
        assert_eq!(scroll_after(Some((10, 0, 3)), -10.0 * row), -10.0 * row);
        // At the start of the list nothing moves.
        assert_eq!(scroll_after(Some((0, 0, 2)), 0.0), 0.0);
    }

    #[test]
    fn scrollstand_nie_ueber_den_anfang() {
        let row = ROW_HEIGHT;
        // A search shrinks the list from row 1 on: never above the start.
        assert_eq!(scroll_after(Some((1, 50, 3)), -20.0 * row), 0.0);
        assert_eq!(scroll_after(None, -5.0 * row), -5.0 * row);
        assert_eq!(scroll_after(None, 3.0), 0.0);
    }

    // --- splice (feature 012) ---

    #[test]
    fn splice_gleiche_liste() {
        assert_eq!(splice(&[1, 2, 3], &[1, 2, 3]), None);
        assert_eq!(splice::<u8>(&[], &[]), None);
    }

    #[test]
    fn splice_bereiche() {
        // One row changed.
        assert_eq!(splice(&[1, 2, 3, 4], &[1, 9, 3, 4]), Some((1, 1, 1)));
        // Three rows inserted in the middle.
        assert_eq!(splice(&[1, 2, 6], &[1, 2, 3, 4, 5, 6]), Some((2, 0, 3)));
        // Two removed at the end.
        assert_eq!(splice(&[1, 2, 3, 4], &[1, 2]), Some((2, 2, 0)));
        // Everything different.
        assert_eq!(splice(&[1, 2], &[3, 4, 5]), Some((0, 2, 3)));
        // Empty old or new list.
        assert_eq!(splice(&[], &[1, 2]), Some((0, 0, 2)));
        assert_eq!(splice(&[1, 2], &[]), Some((0, 2, 0)));
        // Inserting an element equal to the following one: beginning and end do not overlap.
        assert_eq!(splice(&[1, 2], &[1, 2, 2]), Some((2, 0, 1)));
        assert_eq!(splice(&[2, 2], &[2]), Some((1, 1, 0)));
    }

    #[test]
    fn splice_ergibt_die_neue_liste() {
        let cases: [(&[u8], &[u8]); 5] = [
            (&[1, 2, 3], &[1, 3]),
            (&[5, 5, 5], &[5, 5, 5, 5]),
            (&[1, 2, 3, 4, 5], &[1, 7, 8, 5]),
            (&[], &[1]),
            (&[9], &[]),
        ];
        for (old, new) in cases {
            let (start, removed, inserted) = splice(old, new).unwrap();
            let mut result = old.to_vec();
            result.splice(start..start + removed, new[start..start + inserted].iter().copied());
            assert_eq!(result, new, "{old:?} → {new:?}");
        }
    }

    // --- Connection details and own IP address ---

    fn at(seconds: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(seconds)
    }

    #[test]
    fn zeitstempel_wie_go_sie_schreibt() {
        assert_eq!(parse_rfc3339("1970-01-01T00:00:00Z"), Some(at(0)));
        assert_eq!(parse_rfc3339("2026-11-09T12:34:56Z"), Some(at(1_794_227_696)));
        assert_eq!(
            parse_rfc3339("2026-11-09T13:34:56.123456789+01:00"),
            Some(at(1_794_227_696))
        );
        assert_eq!(parse_rfc3339("2026-11-09T11:34:56-01:00"), Some(at(1_794_227_696)));
        assert_eq!(parse_rfc3339("2024-02-29T00:00:00Z"), Some(at(1_709_164_800)));
        for bad in [
            "",
            "2026-11-09",
            "2026-13-01T00:00:00Z",
            "2026-11-09T12:34:56",
            "2026-11-09T12:34:56.Z",
            "2026-11-09T12:34:56+0100",
            "x026-11-09T12:34:56Z",
        ] {
            assert_eq!(parse_rfc3339(bad), None, "{bad}");
        }
    }

    #[test]
    fn dauer_und_datenmenge_auf_deutsch() {
        assert_eq!(duration_text(Duration::from_secs(59), Language::De), "unter 1 Min.");
        assert_eq!(duration_text(Duration::from_secs(180), Language::De), "3 Min.");
        assert_eq!(duration_text(Duration::from_mins(64), Language::De), "1 Std. 4 Min.");
        assert_eq!(bytes_text(999, Language::De), "999 B");
        assert_eq!(bytes_text(1500, Language::De), "1,5 kB");
        assert_eq!(bytes_text(12_345_678, Language::De), "12,3 MB");
        assert_eq!(bytes_text(2_000_000_000, Language::De), "2,0 GB");
        assert_eq!(bytes_text(999_999, Language::De), "999,9 kB");
    }

    fn connected_details() -> ServiceState {
        ServiceState {
            code: "connected".to_owned(),
            host: Some("nl.hideservers.net".to_owned()),
            rx: Some(12_345_678),
            tx: Some(2_345_678),
            since: Some("2026-11-09T12:00:00Z".to_owned()),
            addresses: vec!["10.128.0.2/32".to_owned(), "fd00::2/128".to_owned()],
            dns: vec!["10.128.0.1".to_owned()],
            ..ServiceState::default()
        }
    }

    #[test]
    fn details_nur_bei_verbunden() {
        let state = connected_details();
        let connected = DisplayState::Connected {
            server: state.host.clone(),
        };
        let now = parse_rfc3339("2026-11-09T13:04:30Z").unwrap();
        let details = connection_details(Some(&connected), Some(&state), now, Language::De);
        assert_eq!(
            details,
            ConnectionDetails {
                duration: "1 Std. 4 Min.".to_owned(),
                received: "12,3 MB".to_owned(),
                sent: "2,3 MB".to_owned(),
                address_v4: "10.128.0.2".to_owned(),
                address_v6: "fd00::2".to_owned(),
                dns_v4: "10.128.0.1".to_owned(),
                dns_v6: String::new(),
            }
        );
        for display in [
            DisplayState::NotConnected,
            DisplayState::Blocked,
            DisplayState::Reapplying { back: false },
        ] {
            assert!(connection_details(Some(&display), Some(&state), now, Language::De).is_empty());
        }
        assert!(connection_details(Some(&connected), None, now, Language::De).is_empty());
    }

    /// Several addresses of one kind each stand in a line of their own (feature 012).
    #[test]
    fn mehrere_adressen_untereinander() {
        let state = ServiceState {
            dns: vec!["10.128.0.1".to_owned(), "10.128.0.2".to_owned(), "fd00::1".to_owned()],
            ..connected_details()
        };
        let now = parse_rfc3339("2026-11-09T13:04:30Z").unwrap();
        let details = service_details(&state, now, Language::De);
        assert_eq!(details.dns_v4, "10.128.0.1\n10.128.0.2");
        assert_eq!(details.dns_v6, "fd00::1");
    }

    #[test]
    fn oeffentliche_adresse() {
        let start = Instant::now();
        let mut ip = ExternalIp::Idle;
        assert!(ip.due(start));
        // Without a query no report is accepted.
        ip.answer(Some("203.0.113.7"), None, start);
        assert_eq!(ip, ExternalIp::Idle);
        ip = ExternalIp::Asking { since: start };
        assert!(!ip.due(start));
        assert_eq!(ip.column(false, Language::De), "wird ermittelt …");
        assert!(!ip.expire(start + Duration::from_secs(9)));
        ip.answer(Some("203.0.113.7"), None, start);
        assert_eq!(
            (ip.column(false, Language::De), ip.column(true, Language::De)),
            ("203.0.113.7".to_owned(), "–".to_owned())
        );
        assert!(!ip.due(start + Duration::from_secs(3600)));
        let mut empty = ExternalIp::Asking { since: start };
        empty.answer(Some(""), Some("kaputt"), start);
        assert_eq!(empty, ExternalIp::Failed { at: start });
        assert_eq!(empty.column(true, Language::De), "nicht ermittelbar");
        assert!(!empty.due(start + Duration::from_secs(29)));
        assert!(empty.due(start + Duration::from_secs(30)));
        let mut late = ExternalIp::Asking { since: start };
        assert!(late.expire(start + Duration::from_secs(10)));
        assert!(matches!(late, ExternalIp::Failed { .. }));
    }

    // --- Menu of the tray icon (feature 014) ---

    /// The states of the display the menu must follow, each with a name for the messages.
    fn menu_states() -> Vec<(&'static str, Monitor)> {
        let mut result = Vec::new();
        let mut monitor = Monitor::new();
        state(&mut monitor, "clean", None, false);
        result.push(("nicht verbunden", monitor.clone()));
        monitor.begin_connect("de.hideservers.net").unwrap();
        result.push(("verbinde", monitor));
        let monitor = connected();
        result.push(("verbunden", monitor.clone()));
        let mut switching = monitor.clone();
        switching.begin_connect("de.hideservers.net").unwrap();
        result.push(("wechsel", switching));
        let mut disconnecting = monitor.clone();
        disconnecting.begin_disconnect(false);
        result.push(("trenne", disconnecting));
        let mut reapplying = monitor;
        reapplying.begin_reapply().unwrap();
        result.push(("neuverbinden", reapplying));
        let mut blocked = Monitor::new();
        state(&mut blocked, "routed", None, true);
        result.push(("gesperrt", blocked.clone()));
        blocked.begin_connect("de.hideservers.net").unwrap();
        result.push(("verbinde aus der sperre", blocked));
        for (name, error, routes) in [
            ("dienst nicht erreichbar", Error::NotRunning, false),
            ("dienst weg, gesperrt", Error::NotRunning, true),
            ("keine berechtigung", Error::PermissionDenied, false),
        ] {
            let mut monitor = Monitor::new();
            monitor.observe(Observation::Failed { error, blocked: routes });
            result.push((name, monitor));
        }
        result.push(("vor der ersten beobachtung", Monitor::new()));
        // Start of the service (feature 015).
        let unreachable = |routes| {
            let mut monitor = Monitor::new();
            monitor.observe(Observation::Failed {
                error: Error::NotRunning,
                blocked: routes,
            });
            monitor
        };
        let mut without = unreachable(false);
        without.without_start();
        result.push(("dienst nicht erreichbar, ohne start", without));
        for (name, routes) in [
            ("dienst wird gestartet", false),
            ("dienst wird gestartet, gesperrt", true),
        ] {
            let mut starting = unreachable(routes);
            assert!(starting.request_start(Some("best".into())));
            result.push((name, starting));
        }
        let mut password = unreachable(false);
        assert!(password.request_start(None));
        assert!(password.startup_decided(crate::unit::Before::Start, crate::unit::GroupCheck::Member));
        password.start_answered(Err(crate::unit::StartFailure::NeedsPassword));
        result.push(("start braucht passwort", password));
        result
    }

    /// States in which connecting starts the service first.
    const STARTS_SERVICE: [&str; 2] = ["dienst nicht erreichbar", "dienst weg, gesperrt"];

    fn tray_ctx<'a>(
        monitor: &'a Monitor,
        view: &'a StatusView,
        countries: &'a [Country],
        streaming: &'a [StreamingServer],
        favorites: &'a [String],
        language: Language,
    ) -> TrayContext<'a> {
        TrayContext {
            display: monitor.display(),
            view,
            kill_switch: true,
            countries,
            streaming,
            favorites,
            list: ListState::Ready,
            free_only: false,
            selected: "",
            language,
            update: None,
        }
    }

    /// The countries of all continents, in the order of the menu.
    fn flat_countries(menu: &TrayMenu) -> Vec<MenuCountry> {
        match &menu.locations {
            MenuLocations::Ready(continents) => continents
                .iter()
                .flat_map(|continent| continent.countries.iter().cloned())
                .collect(),
            _ => panic!("list not ready"),
        }
    }

    /// Every text of the menu, also in the submenus.
    fn menu_texts(menu: &TrayMenu) -> Vec<String> {
        let mut texts = vec![
            menu.status.clone(),
            menu.main.label.clone(),
            menu.favorite_toggle.label.clone(),
        ];
        texts.extend(menu.side.iter().map(|side| side.label.clone()));
        texts.extend(menu.update.iter().cloned());
        texts.extend(menu.favorites.iter().map(|place| place.label.clone()));
        texts.extend(menu.streaming.iter().map(|place| place.label.clone()));
        if let MenuLocations::Ready(continents) = &menu.locations {
            texts.extend(continents.iter().map(|continent| continent.label.clone()));
            for country in &flat_countries(menu) {
                texts.push(country.label.clone());
                texts.extend(country.places.iter().map(|place| place.label.clone()));
            }
        }
        texts
    }

    fn all_places(menu: &TrayMenu) -> Vec<MenuPlace> {
        let mut places = menu.streaming.clone();
        if let MenuLocations::Ready(_) = &menu.locations {
            for country in &flat_countries(menu) {
                places.push(MenuPlace {
                    label: country.label.clone(),
                    host: country.host.clone(),
                    enabled: country.enabled,
                    current: country.current,
                });
                places.extend(country.places.iter().cloned());
            }
        }
        places
    }

    #[test]
    fn menue_folgt_seitenleiste_und_hauptknopf_in_jedem_zustand() {
        let countries = real();
        let streaming = streaming::all();
        for language in Language::ALL {
            for (name, monitor) in menu_states() {
                let view = monitor.view();
                let menu = tray_menu(&tray_ctx(&monitor, &view, &countries, &streaming, &[], language));
                let button = main_button(monitor.display(), &view);
                assert_eq!(
                    menu.status,
                    crate::trayicon::short_state(&view, &countries, language),
                    "{name}"
                );
                assert_eq!(menu.main.label, i18n::text(language, button.label), "{name}");
                assert_eq!(menu.main.enabled, button.action != MainAction::None, "{name}");
                match button.side {
                    SideAction::None => assert_eq!(menu.side, None, "{name}"),
                    side => assert_eq!(
                        menu.side.as_ref().map(|entry| entry.label.as_str()),
                        side.label().map(|label| i18n::text(language, label)),
                        "{name}"
                    ),
                }
                let reachable = !matches!(
                    monitor.display(),
                    None | Some(
                        DisplayState::ServiceUnreachable
                            | DisplayState::ServiceUnreachableBlocked
                            | DisplayState::NoPermission
                            | DisplayState::ServiceStarting { .. }
                    )
                );
                assert_eq!(view.start_on_connect, STARTS_SERVICE.contains(&name), "{name}");
                assert_eq!(menu.best_enabled, reachable || view.start_on_connect, "{name}");
            }
        }
    }

    #[test]
    fn menue_texte_der_hauptaktionen() {
        let countries = real();
        let monitor = &menu_states()[0].1;
        let view = monitor.view();
        let menu = tray_menu(&tray_ctx(monitor, &view, &countries, &[], &[], Language::De));
        assert_eq!(menu.status, "Nicht verbunden");
        assert_eq!(
            menu.main,
            MenuEntry {
                label: "Verbinde VPN".into(),
                enabled: true
            }
        );
        assert_eq!(
            menu.side,
            Some(MenuEntry {
                label: "Internet sperren".into(),
                enabled: true
            })
        );
        let monitor = connected();
        let view = monitor.view();
        let menu = tray_menu(&tray_ctx(&monitor, &view, &countries, &[], &[], Language::En));
        assert_eq!(
            menu.main,
            MenuEntry {
                label: "Disconnect".into(),
                enabled: true
            }
        );
        assert_eq!(menu.side, None);
        let mut blocked = Monitor::new();
        state(&mut blocked, "routed", None, true);
        let view = blocked.view();
        let menu = tray_menu(&tray_ctx(&blocked, &view, &countries, &[], &[], Language::De));
        assert_eq!(menu.status, "Internet gesperrt");
        assert_eq!(
            menu.side,
            Some(MenuEntry {
                label: "Sperre aufheben".into(),
                enabled: true
            })
        );
    }

    #[test]
    fn sperren_im_menue_nur_mit_eingeschalteter_internet_sperre() {
        let countries = real();
        let monitor = &menu_states()[0].1;
        let view = monitor.view();
        let mut ctx = tray_ctx(monitor, &view, &countries, &[], &[], Language::De);
        ctx.kill_switch = false;
        assert_eq!(tray_menu(&ctx).side, None);
        // "Unblock internet" stays offered without the setting.
        let mut blocked = Monitor::new();
        state(&mut blocked, "routed", None, true);
        let view = blocked.view();
        let mut ctx = tray_ctx(&blocked, &view, &countries, &[], &[], Language::De);
        ctx.kill_switch = false;
        assert!(tray_menu(&ctx).side.is_some());
    }

    #[test]
    fn bester_standort_im_menue_nur_mit_geladener_liste() {
        let countries = real();
        let monitor = &menu_states()[0].1;
        let view = monitor.view();
        let mut ctx = tray_ctx(monitor, &view, &countries, &[], &[], Language::De);
        assert!(tray_menu(&ctx).best_enabled);
        ctx.list = ListState::Loading;
        assert!(!tray_menu(&ctx).best_enabled);
        ctx.list = ListState::Missing;
        assert!(!tray_menu(&ctx).best_enabled);
        ctx.list = ListState::Ready;
        ctx.countries = &[];
        assert!(!tray_menu(&ctx).best_enabled);
    }

    fn decide(action: &MenuAction, monitor: &Monitor, dialog_open: bool, selected: &str) -> MenuDecision {
        let view = monitor.view();
        menu_decision(
            action,
            &MenuState {
                display: monitor.display(),
                view: &view,
                dialog_open,
                selected,
                kill_switch: true,
            },
        )
    }

    #[test]
    fn offene_rueckfrage_holt_nur_das_fenster() {
        for (name, monitor) in menu_states() {
            for action in [
                MenuAction::Main,
                MenuAction::Best,
                MenuAction::Side,
                MenuAction::Choose("de.hideservers.net".into()),
                MenuAction::ToggleFavorite,
                MenuAction::OpenUpdate,
                MenuAction::About,
            ] {
                assert_eq!(
                    decide(&action, &monitor, true, ""),
                    MenuDecision::BringBack,
                    "{name} {action:?}"
                );
            }
            assert_eq!(
                decide(&MenuAction::ShowWindow, &monitor, true, ""),
                MenuDecision::BringBack
            );
            assert_eq!(
                decide(&MenuAction::Quit, &monitor, true, ""),
                MenuDecision::Quit,
                "{name}"
            );
        }
    }

    #[test]
    fn hauptknopf_im_menue_entscheidet_im_aktuellen_zustand() {
        let states = menu_states();
        let find = |wanted: &str| &states.iter().find(|(name, _)| *name == wanted).unwrap().1;
        let idle = find("nicht verbunden");
        assert_eq!(
            decide(&MenuAction::Main, idle, false, "at.hideservers.net"),
            MenuDecision::Connect("at.hideservers.net".into())
        );
        assert_eq!(
            decide(&MenuAction::Main, idle, false, ""),
            MenuDecision::Connect("best".into())
        );
        assert_eq!(
            decide(&MenuAction::Main, find("verbunden"), false, ""),
            MenuDecision::Disconnect
        );
        assert_eq!(
            decide(&MenuAction::Main, find("gesperrt"), false, "best"),
            MenuDecision::Connect("best".into())
        );
        // A stale "Disconnect" during a switch, or any entry during a transition, does nothing.
        for name in [
            "wechsel",
            "verbinde",
            "trenne",
            "neuverbinden",
            "verbinde aus der sperre",
        ] {
            assert_eq!(
                decide(&MenuAction::Main, find(name), false, ""),
                MenuDecision::Nothing,
                "{name}"
            );
        }
        // Without an answering service, connecting starts it first where that is possible.
        for name in STARTS_SERVICE {
            assert_eq!(
                decide(&MenuAction::Main, find(name), false, ""),
                MenuDecision::StartThenConnect { target: "best".into() },
                "{name}"
            );
            assert_eq!(
                decide(&MenuAction::Main, find(name), false, "at.hideservers.net"),
                MenuDecision::StartThenConnect {
                    target: "at.hideservers.net".into()
                },
                "{name}"
            );
            assert_eq!(
                decide(&MenuAction::Best, find(name), false, "x"),
                MenuDecision::StartThenConnect { target: "best".into() },
                "{name}"
            );
        }
        for name in [
            "keine berechtigung",
            "vor der ersten beobachtung",
            "dienst nicht erreichbar, ohne start",
            "dienst wird gestartet",
            "dienst wird gestartet, gesperrt",
            "start braucht passwort",
        ] {
            assert_eq!(
                decide(&MenuAction::Main, find(name), false, ""),
                MenuDecision::Nothing,
                "{name}"
            );
            assert_eq!(
                decide(&MenuAction::Best, find(name), false, ""),
                MenuDecision::Nothing,
                "{name}"
            );
        }
        assert_eq!(
            decide(&MenuAction::Best, idle, false, "x"),
            MenuDecision::Connect("best".into())
        );
        assert_eq!(
            decide(&MenuAction::Best, find("verbunden"), false, ""),
            MenuDecision::Connect("best".into())
        );
    }

    #[test]
    fn sperr_eintrag_entscheidet_im_aktuellen_zustand() {
        let states = menu_states();
        let find = |wanted: &str| &states.iter().find(|(name, _)| *name == wanted).unwrap().1;
        assert_eq!(
            decide(&MenuAction::Side, find("nicht verbunden"), false, ""),
            MenuDecision::Route
        );
        assert_eq!(
            decide(&MenuAction::Side, find("gesperrt"), false, ""),
            MenuDecision::AskUnblock
        );
        assert_eq!(
            decide(&MenuAction::Side, find("verbinde aus der sperre"), false, ""),
            MenuDecision::AskUnblock
        );
        assert_eq!(
            decide(&MenuAction::Side, find("verbunden"), false, ""),
            MenuDecision::Nothing
        );
        let monitor = find("nicht verbunden");
        let view = monitor.view();
        let without = MenuState {
            display: monitor.display(),
            view: &view,
            dialog_open: false,
            selected: "",
            kill_switch: false,
        };
        assert_eq!(menu_decision(&MenuAction::Side, &without), MenuDecision::Nothing);
    }

    #[test]
    fn standortwahl_und_favorit_im_menue() {
        for (name, monitor) in menu_states() {
            let expected = if service_reachable(monitor.display()) {
                MenuDecision::Connect("at.hideservers.net".into())
            } else if monitor.view().start_on_connect {
                MenuDecision::StartThenConnect {
                    target: "at.hideservers.net".into(),
                }
            } else {
                MenuDecision::Nothing
            };
            assert_eq!(
                decide(&MenuAction::Choose("at.hideservers.net".into()), &monitor, false, ""),
                expected,
                "{name}"
            );
        }
        let states = menu_states();
        let find = |wanted: &str| &states.iter().find(|(name, _)| *name == wanted).unwrap().1;
        assert_eq!(
            decide(
                &MenuAction::ToggleFavorite,
                find("verbunden"),
                false,
                "at.hideservers.net"
            ),
            MenuDecision::ToggleFavorite("nl.hideservers.net".into())
        );
        assert_eq!(
            decide(
                &MenuAction::ToggleFavorite,
                find("nicht verbunden"),
                false,
                "at.hideservers.net"
            ),
            MenuDecision::ToggleFavorite("at.hideservers.net".into())
        );
        assert_eq!(
            decide(&MenuAction::ToggleFavorite, find("nicht verbunden"), false, ""),
            MenuDecision::ToggleFavorite("best".into())
        );
    }

    #[test]
    fn standorte_und_favoriten_nur_mit_erreichbarem_dienst_waehlbar() {
        let countries = real();
        let streaming = streaming::all();
        let favorites = vec!["de.hideservers.net".to_owned(), "best".to_owned()];
        let unreachable = [
            "dienst nicht erreichbar",
            "dienst weg, gesperrt",
            "keine berechtigung",
            "vor der ersten beobachtung",
            "dienst nicht erreichbar, ohne start",
            "dienst wird gestartet",
            "dienst wird gestartet, gesperrt",
            "start braucht passwort",
        ];
        for (name, monitor) in menu_states() {
            let view = monitor.view();
            let menu = tray_menu(&tray_ctx(
                &monitor,
                &view,
                &countries,
                &streaming,
                &favorites,
                Language::De,
            ));
            assert_eq!(
                service_reachable(monitor.display()),
                !unreachable.contains(&name),
                "{name}"
            );
            // Selectable with an answering service, or where connecting starts it first (feature 015).
            let reachable = !unreachable.contains(&name) || STARTS_SERVICE.contains(&name);
            assert_eq!(view.start_on_connect, STARTS_SERVICE.contains(&name), "{name}");
            let list = flat_countries(&menu);
            for country in list {
                if country.places.is_empty() {
                    assert_eq!(country.enabled, reachable, "{name}: {}", country.label);
                } else {
                    // The submenu of a country stays open; only its entries follow the service.
                    assert!(country.enabled, "{name}: {}", country.label);
                    for place in &country.places {
                        assert_eq!(place.enabled, reachable, "{name}: {}", place.label);
                    }
                }
            }
            assert!(!menu.streaming.is_empty());
            for place in &menu.streaming {
                assert_eq!(place.enabled, reachable, "{name}: {}", place.label);
            }
            assert_eq!(menu.favorites.len(), 2);
            for place in &menu.favorites {
                assert_eq!(place.enabled, reachable, "{name}: {}", place.label);
            }
            assert_eq!(menu.best_enabled, reachable, "{name}");
            // Adding or removing a favourite needs no service.
            assert!(menu.favorite_toggle.enabled, "{name}");
        }
    }

    /// Entries of the menu that choose a location (no submenus of countries).
    fn chosen_places(menu: &TrayMenu) -> Vec<MenuPlace> {
        let mut places = menu.streaming.clone();
        places.extend(menu.favorites.iter().cloned());
        if let MenuLocations::Ready(_) = &menu.locations {
            for country in &flat_countries(menu) {
                if country.places.is_empty() {
                    places.push(MenuPlace {
                        label: country.label.clone(),
                        host: country.host.clone(),
                        enabled: country.enabled,
                        current: country.current,
                    });
                }
                places.extend(country.places.iter().cloned());
            }
        }
        assert!(places.len() > 50);
        places
    }

    #[test]
    fn dienst_wird_gestartet_im_fenster_und_im_menue() {
        let countries = real();
        let streaming = streaming::all();
        let favorites = vec!["de.hideservers.net".to_owned()];
        let states = menu_states();
        for (name, blocked) in [
            ("dienst wird gestartet", false),
            ("dienst wird gestartet, gesperrt", true),
        ] {
            let monitor = &states.iter().find(|(state, _)| *state == name).unwrap().1;
            let display = monitor.display();
            assert_eq!(display, Some(&DisplayState::ServiceStarting { blocked }));
            // Busy like connecting, red only with the block.
            assert_eq!(tone(display), if blocked { Tone::Alert } else { Tone::Busy });
            assert_eq!(
                state_color(display),
                if blocked {
                    StateColor::Blocked
                } else {
                    StateColor::Normal
                }
            );
            let view = monitor.view();
            let button = main_button(display, &view);
            assert_eq!(
                (button.label, button.action, button.side),
                (Key::ButtonConnect, MainAction::None, SideAction::None),
                "{name}"
            );
            for language in Language::ALL {
                let menu = tray_menu(&tray_ctx(monitor, &view, &countries, &streaming, &favorites, language));
                let title = if blocked {
                    Key::StateServiceStartingBlockedTitle
                } else {
                    Key::StateServiceStartingTitle
                };
                assert_eq!(menu.status, i18n::text(language, title));
                assert!(!contains_host_or_address(&menu.status), "{}", menu.status);
                assert!(!menu.main.enabled && !menu.best_enabled);
                assert!(chosen_places(&menu).iter().all(|place| !place.enabled));
                assert!(menu.favorites.iter().all(|place| !place.enabled));
                assert!(menu.favorite_toggle.enabled);
            }
            for action in [
                MenuAction::Main,
                MenuAction::Best,
                MenuAction::Choose("at.hideservers.net".into()),
            ] {
                assert_eq!(decide(&action, monitor, false, ""), MenuDecision::Nothing, "{name}");
            }
            assert_eq!(
                decide(&MenuAction::ShowWindow, monitor, false, ""),
                MenuDecision::BringBack
            );
            assert_eq!(decide(&MenuAction::About, monitor, false, ""), MenuDecision::ShowAbout);
            assert_eq!(decide(&MenuAction::Quit, monitor, false, ""), MenuDecision::Quit);
            assert!(matches!(
                decide(&MenuAction::ToggleFavorite, monitor, false, ""),
                MenuDecision::ToggleFavorite(_)
            ));
            assert_eq!(connect_wish(display, &view, "best"), ConnectWish::Nothing);
        }
    }

    #[test]
    fn gruende_sperren_das_verbinden_und_nennen_ihren_titel() {
        use crate::unit::{Before, GroupCheck, StartFailure};
        let countries = real();
        let streaming = streaming::all();
        let unreachable = || {
            let mut monitor = Monitor::new();
            monitor.observe(Observation::Failed {
                error: Error::NotRunning,
                blocked: false,
            });
            monitor
        };
        let mut cases = Vec::new();
        let mut password = unreachable();
        password.request_start(None);
        password.startup_decided(Before::Start, GroupCheck::Member);
        password.start_answered(Err(StartFailure::NeedsPassword));
        cases.push((password, Key::StartNeedsPasswordTitle));
        for (before, title) in [
            (Before::NotInstalled, Key::StartNotInstalledTitle),
            (Before::NoSystemBus, Key::StateServiceUnreachableTitle),
            (Before::NoPermission, Key::StateNoPermissionTitle),
        ] {
            let mut monitor = unreachable();
            monitor.request_start(None);
            monitor.startup_decided(before, GroupCheck::Member);
            cases.push((monitor, title));
        }
        for (mut monitor, title) in cases {
            monitor.observe(Observation::Failed {
                error: Error::NotRunning,
                blocked: false,
            });
            let view = monitor.view();
            assert!(!view.start_on_connect, "{title:?}");
            assert_eq!(
                connect_wish(monitor.display(), &view, "best"),
                ConnectWish::Nothing,
                "{title:?}"
            );
            assert_eq!(
                main_button(monitor.display(), &view).action,
                MainAction::None,
                "{title:?}"
            );
            for language in Language::ALL {
                let menu = tray_menu(&tray_ctx(&monitor, &view, &countries, &streaming, &[], language));
                assert_eq!(menu.status, i18n::text(language, title), "{title:?}");
                assert!(!menu.status.contains('/') && !contains_host_or_address(&menu.status));
                assert!(!menu.main.enabled && !menu.best_enabled, "{title:?}");
                assert!(chosen_places(&menu).iter().all(|place| !place.enabled));
            }
            for action in [
                MenuAction::Main,
                MenuAction::Best,
                MenuAction::Choose("at.hideservers.net".into()),
            ] {
                assert_eq!(decide(&action, &monitor, false, ""), MenuDecision::Nothing, "{title:?}");
            }
            assert_eq!(
                decide(&MenuAction::ShowWindow, &monitor, false, ""),
                MenuDecision::BringBack
            );
            assert_eq!(decide(&MenuAction::About, &monitor, false, ""), MenuDecision::ShowAbout);
            assert_eq!(decide(&MenuAction::Quit, &monitor, false, ""), MenuDecision::Quit);
        }
    }

    #[test]
    fn fenster_info_und_beenden_im_menue() {
        for (name, monitor) in menu_states() {
            assert_eq!(
                decide(&MenuAction::ShowWindow, &monitor, false, ""),
                MenuDecision::BringBack,
                "{name}"
            );
            assert_eq!(
                decide(&MenuAction::About, &monitor, false, ""),
                MenuDecision::ShowAbout,
                "{name}"
            );
            assert_eq!(
                decide(&MenuAction::Quit, &monitor, false, ""),
                MenuDecision::Quit,
                "{name}"
            );
            assert_eq!(
                decide(&MenuAction::OpenUpdate, &monitor, false, ""),
                MenuDecision::OpenUpdate,
                "{name}"
            );
        }
    }

    #[test]
    fn favoriten_im_menue_wie_lieblingsstandorte() {
        let countries = real();
        let streaming = streaming::all();
        let favorites: Vec<String> = [
            "at.hideservers.net",
            "streaming-jp.hideservers.net",
            "best",
            "fi.hideservers.net",
            "gone.hideservers.net",
        ]
        .map(str::to_owned)
        .to_vec();
        let monitor = &menu_states()[0].1;
        let view = monitor.view();
        let menu = tray_menu(&tray_ctx(
            monitor,
            &view,
            &countries,
            &streaming,
            &favorites,
            Language::De,
        ));
        let labels: Vec<&str> = menu.favorites.iter().map(|place| place.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "Österreich, Wien",
                "Japan (Streaming)",
                "Bester Standort",
                "Finnland, Automatisch",
                "Standort nicht verfügbar"
            ]
        );
        let items = favorite_items(&favorites, &countries, &streaming, false, None, Language::De);
        assert_eq!(items[0].label, labels[0], "gleicher Name wie in „Lieblingsstandorte“");
        let enabled: Vec<bool> = menu.favorites.iter().map(|place| place.enabled).collect();
        assert_eq!(enabled, [true, true, true, true, false]);
        assert!(menu.favorites.iter().all(|place| !place.current));
        let menu = tray_menu(&tray_ctx(
            monitor,
            &view,
            &countries,
            &streaming,
            &favorites,
            Language::En,
        ));
        assert_eq!(menu.favorites[1].label, "Japan (Streaming)");
        assert_eq!(menu.favorites[2].label, "Best location");
        // Loading without a list yet: no host names.
        let mut ctx = tray_ctx(monitor, &view, &[], &streaming, &favorites, Language::De);
        ctx.list = ListState::Loading;
        let menu = tray_menu(&ctx);
        assert_eq!(menu.favorites[0].label, "Wird geladen …");
        assert!(!menu.favorites[0].enabled);
        // No favourites: an empty list, Slint shows "No favorites".
        let menu = tray_menu(&tray_ctx(monitor, &view, &countries, &streaming, &[], Language::De));
        assert!(menu.favorites.is_empty());
    }

    #[test]
    fn bezahl_favorit_bei_nur_kostenlosen_nicht_waehlbar() {
        let countries = real();
        let favorites = vec!["at.hideservers.net".to_owned(), "free-de.hideservers.net".to_owned()];
        let monitor = &menu_states()[0].1;
        let view = monitor.view();
        let mut ctx = tray_ctx(monitor, &view, &countries, &[], &favorites, Language::De);
        ctx.free_only = true;
        let menu = tray_menu(&ctx);
        assert_eq!(
            menu.favorites.iter().map(|place| place.enabled).collect::<Vec<_>>(),
            [false, true]
        );
    }

    #[test]
    fn favorit_umschalten_bezieht_sich_auf_den_aktuellen_standort() {
        let countries = real();
        let favorites = vec!["nl.hideservers.net".to_owned()];
        let connected = connected();
        let view = connected.view();
        let menu = tray_menu(&tray_ctx(&connected, &view, &countries, &[], &favorites, Language::De));
        assert_eq!(menu.favorite_toggle.label, "Aktuellen Standort aus Favoriten entfernen");
        let menu = tray_menu(&tray_ctx(&connected, &view, &countries, &[], &[], Language::En));
        assert_eq!(menu.favorite_toggle.label, "Add current location to favorites");
        let idle = &menu_states()[0].1;
        let view = idle.view();
        let mut ctx = tray_ctx(idle, &view, &countries, &[], &favorites, Language::De);
        ctx.selected = "nl.hideservers.net";
        assert_eq!(
            tray_menu(&ctx).favorite_toggle.label,
            "Aktuellen Standort aus Favoriten entfernen"
        );
        ctx.selected = "";
        assert_eq!(
            tray_menu(&ctx).favorite_toggle.label,
            "Aktuellen Standort zu Favoriten hinzufügen"
        );
        let best = vec!["best".to_owned()];
        ctx.favorites = &best;
        assert_eq!(
            tray_menu(&ctx).favorite_toggle.label,
            "Aktuellen Standort aus Favoriten entfernen"
        );
    }

    #[test]
    fn standorte_im_menue_wie_der_reiter_standard() {
        let countries = real();
        let streaming = streaming::visible(&streaming::Check::Pending);
        let monitor = &menu_states()[0].1;
        let view = monitor.view();
        for language in Language::ALL {
            let menu = tray_menu(&tray_ctx(monitor, &view, &countries, &streaming, &[], language));
            let menu_countries = flat_countries(&menu);
            assert_eq!(menu_countries.len(), 57);
            let rows = location_items(&ctx(&countries, language));
            let tab: Vec<&str> = rows
                .iter()
                .filter(|row| matches!(row.kind, ItemKind::Location | ItemKind::Country))
                .map(|row| row.country.as_str())
                .collect();
            let shown: Vec<&str> = menu_countries.iter().map(|country| country.label.as_str()).collect();
            let mut shown_sorted = shown.clone();
            shown_sorted.sort_unstable();
            let mut tab_sorted = tab.clone();
            tab_sorted.sort_unstable();
            assert_eq!(shown_sorted, tab_sorted, "jedes Land genau einmal ({language:?})");
            // Within a continent the order is the one of the tab.
            let MenuLocations::Ready(continents) = &menu.locations else {
                panic!("Liste geladen");
            };
            for continent in continents {
                let inside: Vec<&str> = continent
                    .countries
                    .iter()
                    .map(|country| country.label.as_str())
                    .collect();
                let in_tab: Vec<&str> = tab.iter().copied().filter(|name| inside.contains(name)).collect();
                assert_eq!(
                    inside, in_tab,
                    "Sortierung wie im Reiter ({language:?}, {:?})",
                    continent.continent
                );
            }
            for country in menu_countries {
                if country.places.is_empty() {
                    assert!(!country.host.is_empty(), "{}", country.label);
                } else {
                    assert!(country.places.len() >= 2, "{}", country.label);
                }
            }
            assert_eq!(menu.streaming.len(), 20);
            let names: Vec<String> = menu
                .streaming
                .iter()
                .map(|place| names::sort_key(&place.label))
                .collect();
            let mut sorted = names.clone();
            sorted.sort();
            assert_eq!(names, sorted);
        }
        let menu = tray_menu(&tray_ctx(monitor, &view, &countries, &streaming, &[], Language::De));
        let menu_countries = flat_countries(&menu);
        let austria = menu_countries
            .iter()
            .position(|country| country.label == "Österreich")
            .unwrap();
        assert_eq!(menu_countries[austria - 1].label, "Norwegen");
        let germany = menu_countries
            .iter()
            .find(|country| country.label == "Deutschland")
            .unwrap();
        assert_eq!(germany.host, "de.hideservers.net");
        assert_eq!(germany.places[0].label, "Automatisch");
        assert_eq!(germany.places[0].host, "de.hideservers.net");
        let finland = menu_countries
            .iter()
            .find(|country| country.label == "Finnland")
            .unwrap();
        assert_eq!(finland.places[0].host, "fi.hideservers.net", "gebildeter Name");
        // After a failure with the built name it is gone (main removes it from the list).
        let mut without = countries.clone();
        servers::drop_derived(&mut without, &BTreeSet::from(["fi.hideservers.net".to_owned()]));
        let menu = tray_menu(&tray_ctx(monitor, &view, &without, &streaming, &[], Language::De));
        let menu_countries = flat_countries(&menu);
        let finland = menu_countries
            .iter()
            .find(|country| country.label == "Finnland")
            .unwrap();
        assert!(finland.places.iter().all(|place| place.host != "fi.hideservers.net"));
    }

    #[test]
    fn kostenlose_standorte_tragen_ihr_kennzeichen() {
        let countries = real();
        let monitor = &menu_states()[0].1;
        let view = monitor.view();
        let menu = tray_menu(&tray_ctx(monitor, &view, &countries, &[], &[], Language::De));
        let menu_countries = flat_countries(&menu);
        let germany = menu_countries
            .iter()
            .find(|country| country.label == "Deutschland")
            .unwrap();
        let free = germany
            .places
            .iter()
            .find(|place| place.host == "free-de.hideservers.net")
            .unwrap();
        assert!(free.label.ends_with(" (kostenlos)"), "{}", free.label);
        assert_eq!(
            germany
                .places
                .iter()
                .filter(|place| place.label.contains("kostenlos"))
                .count(),
            1
        );
        let menu = tray_menu(&tray_ctx(monitor, &view, &countries, &[], &[], Language::En));
        let menu_countries = flat_countries(&menu);
        let germany = menu_countries
            .iter()
            .find(|country| country.label == "Germany")
            .unwrap();
        assert!(germany.places.iter().any(|place| place.label.ends_with(" (free)")));
    }

    #[test]
    fn standorte_im_menue_mit_nur_kostenlosen() {
        let countries = real();
        let monitor = &menu_states()[0].1;
        let view = monitor.view();
        let mut ctx = tray_ctx(monitor, &view, &countries, &[], &[], Language::En);
        ctx.free_only = true;
        let menu = tray_menu(&ctx);
        let menu_countries = flat_countries(&menu);
        let free: BTreeSet<&str> = countries
            .iter()
            .flat_map(|country| &country.servers)
            .filter(|server| server.free)
            .map(|server| server.host.as_str())
            .collect();
        let hosts: Vec<String> = all_places(&menu)
            .into_iter()
            .filter(|place| !place.host.is_empty())
            .map(|place| place.host)
            .collect();
        assert!(!hosts.is_empty());
        assert!(hosts.iter().all(|host| free.contains(host.as_str())), "{hosts:?}");
        assert!(menu_countries.len() < 57);
    }

    #[test]
    fn standorte_beim_laden_und_ohne_liste() {
        let streaming = streaming::all();
        let monitor = &menu_states()[0].1;
        let view = monitor.view();
        let mut ctx = tray_ctx(monitor, &view, &[], &streaming, &[], Language::De);
        ctx.list = ListState::Loading;
        let menu = tray_menu(&ctx);
        assert_eq!(menu.locations, MenuLocations::Loading);
        assert_eq!(menu.streaming.len(), 20, "eingebaute Liste auch ohne Serverliste");
        ctx.list = ListState::Missing;
        assert_eq!(tray_menu(&ctx).locations, MenuLocations::Missing);
    }

    #[test]
    fn verbundener_standort_genau_einmal_mit_haken() {
        let countries = real();
        let streaming = streaming::all();
        let favorites = vec![
            "nl.hideservers.net".to_owned(),
            "streaming-jp.hideservers.net".to_owned(),
        ];
        for host in [
            "nl.hideservers.net",
            "at.hideservers.net",
            "fi2.hideservers.net",
            "streaming-jp.hideservers.net",
        ] {
            let mut monitor = Monitor::new();
            state(&mut monitor, "connected", Some(host), false);
            let view = monitor.view();
            let menu = tray_menu(&tray_ctx(
                &monitor,
                &view,
                &countries,
                &streaming,
                &favorites,
                Language::De,
            ));
            let marked: Vec<String> = menu_texts(&menu)
                .into_iter()
                .filter(|text| text.starts_with(CURRENT_MARK))
                .collect();
            // The location itself, and for a location of the list also its continent (not for streaming).
            let in_list = !host.starts_with("streaming-");
            assert_eq!(marked.len(), 1 + usize::from(in_list), "{host}: {marked:?}");
            if let MenuLocations::Ready(continents) = &menu.locations {
                let marked_continents = continents.iter().filter(|continent| continent.current).count();
                assert_eq!(marked_continents, usize::from(in_list), "{host}");
            }
            let current: Vec<MenuPlace> = all_places(&menu).into_iter().filter(|place| place.current).collect();
            assert_eq!(current.len(), 1, "{host}");
            assert_eq!(current[0].host, host);
            assert!(
                menu.favorites
                    .iter()
                    .all(|place| !place.label.starts_with(CURRENT_MARK))
            );
        }
        // Without a connection no mark.
        for (name, monitor) in menu_states() {
            if name == "verbunden" || name == "wechsel" || name == "trenne" || name == "neuverbinden" {
                continue;
            }
            let view = monitor.view();
            let menu = tray_menu(&tray_ctx(
                &monitor,
                &view,
                &countries,
                &streaming,
                &favorites,
                Language::En,
            ));
            assert!(
                menu_texts(&menu).iter().all(|text| !text.contains(CURRENT_MARK.trim())),
                "{name}"
            );
        }
    }

    fn continent_menu(
        monitor: &Monitor,
        countries: &[Country],
        language: Language,
        free_only: bool,
    ) -> Vec<MenuContinent> {
        let view = monitor.view();
        let streaming = streaming::visible(&streaming::Check::Pending);
        let mut ctx = tray_ctx(monitor, &view, countries, &streaming, &[], language);
        ctx.free_only = free_only;
        match tray_menu(&ctx).locations {
            MenuLocations::Ready(continents) => continents,
            other => panic!("list not ready: {other:?}"),
        }
    }

    #[test]
    fn locations_are_sorted_under_six_continents_in_both_languages() {
        let countries = real();
        let monitor = &menu_states()[0].1;
        let expected = [
            (Continent::Europe, 31),
            (Continent::Asia, 13),
            (Continent::NorthAmerica, 3),
            (Continent::SouthAmerica, 5),
            (Continent::Africa, 3),
            (Continent::Oceania, 2),
        ];
        for language in Language::ALL {
            let continents = continent_menu(monitor, &countries, language, false);
            let shown: Vec<(Continent, usize)> = continents
                .iter()
                .map(|continent| (continent.continent, continent.countries.len()))
                .collect();
            assert_eq!(shown, expected, "{language:?}");
            for continent in &continents {
                assert_eq!(continent.label, continent.continent.name(language));
                assert!(!continent.current);
                // Sorted by the name in the language in force.
                let keys: Vec<String> = continent
                    .countries
                    .iter()
                    .map(|country| names::sort_key(&country.label))
                    .collect();
                let mut sorted = keys.clone();
                sorted.sort();
                assert_eq!(keys, sorted, "{language:?} {:?}", continent.continent);
            }
            // The longest list is Europe with 31 entries.
            assert_eq!(
                continents.iter().map(|continent| continent.countries.len()).max(),
                Some(31)
            );
            // Every country exactly once.
            let mut codes: Vec<&str> = continents
                .iter()
                .flat_map(|continent| continent.countries.iter().map(|country| country.code.as_str()))
                .collect();
            assert_eq!(codes.len(), 57);
            codes.sort_unstable();
            codes.dedup();
            assert_eq!(codes.len(), 57);
        }
        // The order of the continents is the same in both languages, the countries are sorted anew.
        let en = continent_menu(monitor, &countries, Language::En, false);
        let de = continent_menu(monitor, &countries, Language::De, false);
        let order = |list: &[MenuContinent]| list.iter().map(|continent| continent.continent).collect::<Vec<_>>();
        assert_eq!(order(&en), order(&de));
        let europe = |list: &[MenuContinent]| {
            list[0]
                .countries
                .iter()
                .map(|country| country.label.clone())
                .collect::<Vec<_>>()
        };
        assert_ne!(europe(&en), europe(&de));
    }

    #[test]
    fn countries_stand_under_the_expected_continent() {
        let countries = real();
        let monitor = &menu_states()[0].1;
        let continents = continent_menu(monitor, &countries, Language::En, false);
        let under = |continent: Continent| -> Vec<&str> {
            continents
                .iter()
                .find(|entry| entry.continent == continent)
                .map(|entry| entry.countries.iter().map(|country| country.code.as_str()).collect())
                .unwrap_or_default()
        };
        assert!(under(Continent::Europe).contains(&"TR"));
        assert!(under(Continent::Europe).contains(&"FI"));
        assert!(under(Continent::Asia).contains(&"AE"));
        assert_eq!(under(Continent::NorthAmerica), ["CA", "MX", "US"]);
        assert_eq!(under(Continent::Africa), ["EG", "MA", "ZA"]);
        assert_eq!(under(Continent::Oceania), ["AU", "NZ"]);
    }

    #[test]
    fn free_only_leaves_out_empty_continents() {
        let countries = real();
        let monitor = &menu_states()[0].1;
        for language in Language::ALL {
            let continents = continent_menu(monitor, &countries, language, true);
            let shown: Vec<Continent> = continents.iter().map(|continent| continent.continent).collect();
            assert_eq!(shown, [Continent::Europe, Continent::NorthAmerica], "{language:?}");
        }
    }

    #[test]
    fn menu_continents_leaves_out_empty_ones_and_puts_unknown_countries_into_europe() {
        let country = |code: &str| MenuCountry {
            label: code.to_owned(),
            code: code.to_owned(),
            host: format!("{code}.example"),
            enabled: true,
            current: false,
            places: Vec::new(),
        };
        let assignment = BTreeMap::from([
            ("JP".to_owned(), Continent::Asia),
            ("AU".to_owned(), Continent::Oceania),
        ]);
        let continents = menu_continents(
            vec![country("AU"), country("JP"), country("ZZ")],
            &assignment,
            Language::En,
        );
        let shown: Vec<(Continent, usize)> = continents
            .iter()
            .map(|continent| (continent.continent, continent.countries.len()))
            .collect();
        assert_eq!(
            shown,
            [(Continent::Europe, 1), (Continent::Asia, 1), (Continent::Oceania, 1)]
        );
        assert_eq!(continents[0].countries[0].code, "ZZ");
        assert!(menu_continents(Vec::new(), &assignment, Language::En).is_empty());
    }

    #[test]
    fn the_connected_location_marks_its_continent_and_itself() {
        let countries = real();
        let monitor = menu_states()
            .into_iter()
            .find(|(name, _)| *name == "verbunden")
            .map(|(_, monitor)| monitor)
            .unwrap();
        let host = monitor.view().connected_host;
        assert!(!host.is_empty());
        let owner = countries
            .iter()
            .find(|country| country.servers.iter().any(|server| server.host == host))
            .expect("the connected server is in the list");
        for language in Language::ALL {
            let continents = continent_menu(&monitor, &countries, language, false);
            let marked_continents: Vec<&MenuContinent> =
                continents.iter().filter(|continent| continent.current).collect();
            assert_eq!(marked_continents.len(), 1, "{language:?}");
            let continent = marked_continents[0];
            assert_eq!(
                continent.label,
                format!("{CURRENT_MARK}{}", continent.continent.name(language))
            );
            assert!(continent.countries.iter().any(|entry| entry.code == owner.code));
            let marks: usize = continent
                .countries
                .iter()
                .map(|entry| {
                    usize::from(entry.label.starts_with(CURRENT_MARK))
                        + entry
                            .places
                            .iter()
                            .filter(|place| place.label.starts_with(CURRENT_MARK))
                            .count()
                })
                .sum();
            assert_eq!(marks, 1, "{language:?}");
            for other in continents.iter().filter(|other| !other.current) {
                assert_eq!(other.label, other.continent.name(language));
            }
        }
    }

    #[test]
    fn abschnitte_halten_die_reihenfolge() {
        let country = |label: &str, places: usize| MenuCountry {
            label: label.into(),
            code: String::new(),
            host: label.into(),
            enabled: true,
            current: false,
            places: (0..places)
                .map(|index| MenuPlace {
                    label: format!("{label}{index}"),
                    host: format!("{label}{index}"),
                    enabled: true,
                    current: false,
                })
                .collect(),
        };
        let list = vec![
            country("A", 0),
            country("B", 0),
            country("C", 2),
            country("D", 3),
            country("E", 0),
            country("F", 2),
        ];
        let slots = menu_slots(&list, 12);
        let flat: Vec<&str> = slots
            .iter()
            .flat_map(|slot| slot.singles.iter().chain(&slot.groups))
            .map(|country| country.label.as_str())
            .collect();
        assert_eq!(flat, ["A", "B", "C", "D", "E", "F"]);
        assert_eq!(slots.len(), 2);
        // Starting with a group: the first section has no singles.
        let slots = menu_slots(&[country("C", 2), country("A", 0)], 12);
        assert!(slots[0].singles.is_empty());
        assert_eq!(slots[1].singles[0].label, "A");
        // Too many changes: the rest goes into the last section, nothing is lost.
        let many: Vec<MenuCountry> = (0..30)
            .map(|index| country(&format!("L{index:02}"), index % 2 * 2))
            .collect();
        let slots = menu_slots(&many, 3);
        assert_eq!(slots.len(), 3);
        let count: usize = slots.iter().map(|slot| slot.singles.len() + slot.groups.len()).sum();
        assert_eq!(count, 30);
        assert_eq!(menu_slots(&[], 12).len(), 1);
    }

    #[test]
    fn echte_liste_passt_in_die_abschnitte() {
        let countries = real();
        let monitor = &menu_states()[0].1;
        let view = monitor.view();
        for language in Language::ALL {
            let menu = tray_menu(&tray_ctx(monitor, &view, &countries, &[], &[], language));
            let MenuLocations::Ready(continents) = &menu.locations else {
                panic!("list not ready");
            };
            for continent in continents {
                // One section more than needed shows that the order stays complete.
                let needed = menu_slots(&continent.countries, usize::MAX).len();
                assert!(needed < MENU_SLOTS, "{language:?} {:?}: {needed}", continent.continent);
                assert_eq!(menu_slots(&continent.countries, MENU_SLOTS).len(), needed);
            }
        }
    }

    #[test]
    fn host_oder_adresse_wird_erkannt() {
        for text in [
            "berlin.hideservers.net",
            "Verbunden mit de.hideservers.net.",
            "(nl.hideservers.net)",
            "192.0.2.10",
            "Adresse 198.51.100.7, gesperrt",
            "2001:0db8:85a3:0000:0000:8a2e:0370:7334",
            "fe80::1",
            "github.com",
        ] {
            assert!(contains_host_or_address(text), "{text}");
        }
        for text in [
            "Deutschland, Berlin",
            "1.5 GB",
            "1,5 GB",
            "Celata 0.9.0 ist verfügbar",
            "Version 0.8.1",
            "Verbunden.",
            "Frankfurt #2",
            "12:30",
            "Sperre aufgehoben",
            "Aktion gescheitert. Einzelheiten im Fenster von Celata.",
            "z.B. so",
            "",
        ] {
            assert!(!contains_host_or_address(text), "{text}");
        }
    }

    #[test]
    fn kein_hostname_und_keine_adresse_im_menue() {
        let countries = real();
        let streaming = streaming::all();
        let favorites: Vec<String> = [
            "at.hideservers.net",
            "de.hideservers.net",
            "streaming-jp.hideservers.net",
            "best",
            "gone.hideservers.net",
            "fi.hideservers.net",
        ]
        .map(str::to_owned)
        .to_vec();
        for language in Language::ALL {
            for (name, monitor) in menu_states() {
                let view = monitor.view();
                for list in [ListState::Ready, ListState::Loading, ListState::Missing] {
                    for free_only in [false, true] {
                        let mut ctx = tray_ctx(&monitor, &view, &countries, &streaming, &favorites, language);
                        ctx.list = list;
                        ctx.free_only = free_only;
                        ctx.update = Some("0.9.0");
                        for text in menu_texts(&tray_menu(&ctx)) {
                            assert!(!contains_host_or_address(&text), "{name} {language:?}: {text}");
                        }
                    }
                }
            }
        }
    }
}
