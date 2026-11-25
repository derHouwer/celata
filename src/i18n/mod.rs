// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! Languages and texts of Celata.
//!
//! Every visible text from Rust is a [`Key`] with one text per language in `en.rs` and `de.rs`. If a
//! text is missing there, the program does not build. Stored messages are [`Phrase`]s and are translated
//! only when they are shown, so that they appear in the new language after a change of language.
//!
//! Texts in `ui/*.slint`, in contrast, are in `@tr("…")` with their translation in `ui/translations/`.

use std::sync::atomic::{AtomicU8, Ordering};

mod de;
mod en;

/// A language of the user interface. The order of [`Language::ALL`] is the order in the selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Language {
    /// Default without a stored choice.
    #[default]
    En,
    De,
}

impl Language {
    /// The only list of the available languages: selection, search and tests read it.
    pub const ALL: [Language; 2] = [Language::En, Language::De];

    /// Code in the settings file and for Slint; equal to the directory name below `ui/translations/`.
    pub fn code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::De => "de",
        }
    }

    /// Name in its own spelling, never translated.
    pub fn native_name(self) -> &'static str {
        match self {
            Self::En => "English",
            Self::De => "Deutsch",
        }
    }

    /// Decimal separator for amounts of data.
    pub fn decimal_separator(self) -> char {
        match self {
            Self::En => '.',
            Self::De => ',',
        }
    }

    /// Language for a code; case and white space do not count.
    pub fn from_code(code: &str) -> Option<Language> {
        let code = code.trim();
        Self::ALL
            .into_iter()
            .find(|language| language.code().eq_ignore_ascii_case(code))
    }

    fn index(self) -> u8 {
        match self {
            Self::En => 0,
            Self::De => 1,
        }
    }
}

/// Decision on a click on a language: `None` for an unknown code or if it means the language in
/// force, otherwise the new language.
pub fn choose(current: Language, code: &str) -> Option<Language> {
    Language::from_code(code).filter(|chosen| *chosen != current)
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

/// Language in force for background threads and messages in the terminal. English before the settings
/// are loaded.
pub fn current() -> Language {
    match CURRENT.load(Ordering::Relaxed) {
        1 => Language::De,
        _ => Language::En,
    }
}

/// Sets the language for [`current`]. Change it only together with the language of the user interface (`main.rs`).
pub fn set_current(language: Language) {
    CURRENT.store(language.index(), Ordering::Relaxed);
}

macro_rules! keys {
    ($($(#[$meta:meta])* $name:ident,)*) => {
        /// Key of a text from Rust. Values appear in the text as `{0}`, `{1}` …
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum Key {
            $($(#[$meta])* $name,)*
        }

        impl Key {
            /// All keys, for the completeness checks.
            pub const ALL: &'static [Key] = &[$(Key::$name,)*];
        }
    };
}

keys! {
    // --- General ---
    /// "{0} Reason: {1}": message with an error appended.
    WithReason,
    /// Error message in the terminal: "Celata: {0}".
    TerminalPrefix,

    // --- Numbers and durations (look.rs) ---
    DurationUnderMinute,
    DurationMinutes,
    DurationHoursMinutes,

    // --- Donation hint (donation.rs) ---
    DonationCardTitle,
    DonationCardText,
    DonationButton,
    DonationButtonLabel,
    DonationSectionTitle,
    DonationSectionText,
    DonationToggle,
    DonationToggleDescription,
    DonationBlockedNotice,
    DonationOpenFailed,

    // --- Settings of Celata ---
    LanguageSaveFailed,
    // --- Terminal (main.rs) ---
    TerminalLedgerSaveFailed,
    /// "A background thread ({0}) …"
    TerminalThreadFailed,
    /// The bundled translation is missing; {0} is the code.
    TerminalTranslationMissing,
    /// The application ID for the desktop could not be set; {0} is the error.
    TerminalAppIdFailed,
    /// The desktop entry or the icon could not be created; {0} is the error.
    TerminalDesktopEntryFailed,
    /// Description in the desktop entry (`Comment`).
    DesktopEntryComment,
    /// The window cannot be opened: neither Wayland nor X11 is reachable. Goes to the terminal.
    NoGraphicalSession,

    // --- Questions (main.rs) ---
    ConfirmRebuildTitle,
    /// Connecting from "Internet blocked" has to rebuild the blocking routes.
    ConfirmRebuildText,
    ConfirmRebuildOk,
    ConfirmSettingTitle,
    ConfirmSettingOk,
    ConfirmResetTitle,
    ConfirmResetOk,
    ResetAdvancedText,
    ResetAllWithFiltersText,
    ResetAllText,

    // --- Settings page (main.rs) ---
    /// {0}: error text of the system, unchanged.
    SettingsSaveFailed,
    SettingsApplying,
    /// {0}: count.
    DohDefault,
    DohNone,
    /// {0}: count.
    DohCustom,

    // --- Server list (servers.rs, main.rs) ---
    ListStale,
    ServerListNetwork,
    ServerListInvalid,

    // --- Categories (categories.rs) ---
    CategoriesNetwork,
    CategoriesInvalid,
    // --- Errors of the service (daemon/http.rs) ---
    ErrorNotRunning,
    ErrorPermissionDenied,
    ErrorBusy,
    ErrorTimeout,
    ErrorConnectionLost,
    /// {0}: HTTP status code.
    ErrorStatus,
    ErrorInvalidResponse,
    ErrorResponseTooLarge,
    /// {0}: error text of the service, unchanged.
    ErrorService,
    ErrorInvalidHost,
    // --- Streaming (streaming.rs) ---
    /// "Streaming {0}" with the country name.
    StreamingName,
    /// {0}: description, {1}: host name.
    StreamingLabel,
    // --- Field errors (settings.rs) ---
    FieldNameError,
    FieldNetworkError,
    FieldDnsError,
    FieldStampError,
    FieldBackupError,
    FieldInterfaceError,
    FieldInterfaceTaken,
    FieldTableReserved,
    /// {0}: smallest, {1}: largest value.
    FieldRange,
    FieldPgError,
    FieldRiskUnknown,
    FieldIllegalUnknown,
    FieldCategoryUnknown,
    FieldNameInBothLists,
    FieldDnsRequired,
    FieldIpVersionUnknown,
    FieldNotSelectable,
    /// {0}: key from the user interface.
    FieldUnknownSetting,

    // --- State and file of the settings (settings.rs) ---
    /// {0}: tested version of the service.
    SettingsVersionNotice,
    /// {0}: notice about the version, {1}: version the service reports.
    UntestedVersion,
    SettingsAppliesNow,
    SettingsAppliesNext,
    SettingsFileBroken,
    SettingsFileNewer,

    // --- Warnings before applying (settings.rs) ---
    WarningRebuild,
    WarningKillSwitch,
    WarningPortForward,
    WarningSplit,
    WarningWideSplit,
    WarningResolver,
    WarningDohOff,
    WarningAdvanced,
    WarningResolvBackup,
    /// Two sentences in a row.
    SentencePair,
    /// Two paragraphs in a row.
    ParagraphPair,

    // --- Hints in the overview (settings.rs) ---
    HintKillSwitchOff,
    /// {0}: networks, comma-separated.
    HintSplitTunnel,
    HintPortForward,
    HintDohOff,
    /// {0}: servers, comma-separated.
    HintPlainDns,
    HintAdvanced,
    /// Shown while `dnsleak::assess` says `AtRisk`; the documentation has a section with this exact name.
    HintDnsLeak,
    // --- States (state.rs) ---
    StateCheckingTitle,
    StateCheckingText,
    StateNotConnectedTitle,
    StateNotConnectedText,
    StateConnectingTitle,
    StateConnectingText,
    StateConnectingBlockedTitle,
    StateConnectingBlockedText,
    /// {0}: server that is connected to (feature 013).
    StateConnectingToTitle,
    /// {0}: server that is connected to, while the internet is blocked (feature 013).
    StateConnectingBlockedToTitle,
    StateConnectedTitle,
    StateConnectedText,
    StateDisconnectingTitle,
    StateDisconnectingText,
    /// {0}: server that is switched to.
    StateSwitchingTitle,
    StateSwitchingText,
    /// {0}: previous server.
    StateReturningTitle,
    StateReturningText,
    StateReapplyingTitle,
    StateReapplyingText,
    StateReapplyingBackTitle,
    StateReapplyingBackText,
    StateBlockedTitle,
    StateBlockedText,
    StateServiceUnreachableTitle,
    StateServiceUnreachableText,
    StateServiceUnreachableBlockedTitle,
    StateServiceUnreachableBlockedText,
    StateNoPermissionTitle,
    StateNoPermissionText,
    StateServiceFaultyTitle,
    StateServiceFaultyText,
    /// {0}: error.
    DetailReason,
    /// {0}: reported, {1}: tested version.
    VersionWarning,

    // --- Question on closing (state.rs) ---
    CloseConnectedText,
    CloseBusyText,
    CloseBlockedText,
    CloseOfflineBlockedText,
    CloseKeepConnected,
    CloseKeepBlocked,
    CloseAnyway,

    // --- Notices after actions (state.rs) ---
    NoticeConnectNotStarted,
    /// {0}: error text of the service.
    NoticeConnectRejected,
    NoticeConnectFailedReleased,
    NoticeConnectFailedBlocked,
    NoticeConnectReleaseFailed,
    /// {0}: error text of the service.
    NoticeRouteRejected,
    NoticeRouteFailed,
    RouteHint,
    NoticeUnblocked,
    NoticeUnblockFailed,
    NoticeCloseDisconnectFailed,
    NoticeDisconnectFailed,
    UnblockWarning,

    // --- Reconnecting after a change of settings (state.rs) ---
    NoticeReapplied,
    NoticeReapplyLater,
    /// {0}: error.
    NoticeReapplyReturned,
    /// {0}: error text of the service.
    NoticeReapplyRejected,
    /// {0}: "They apply from the next connection."
    NoticeReapplyRefused,
    /// {0}: "They apply from the next connection."
    NoticeReapplyUnreachable,
    /// {0}: "They apply from the next connection."
    NoticeReapplyNotStarted,
    NoticeReapplyStranded,
    NoticeReapplyUnclear,
    // --- Switching servers (switch.rs) ---
    /// {0}: new server, {1}: previous server.
    NoticeSwitchReturned,
    /// {0}: previous server.
    NoticeSwitchStranded,
    NoticeSwitchRefused,
    /// {0}: new server, {1}: previous server.
    NoticeSwitchUnreachable,
    NoticeSwitchUnclear,
    /// {0}: error text of the service.
    NoticeSwitchRejected,
    // --- Server list and locations (servers.rs, look.rs) ---
    /// Main entry of a country with several places.
    ServerAutomatic,
    ServerFree,
    ServerPlain,
    AutomaticPlace,
    StreamingPlace,
    /// {0}: country or "Streaming", {1}: place or country.
    LocationChoice,
    /// Headings in the location list (feature 012).
    ListHeadingFavorites,
    ListHeadingAll,
    /// {0}: location, {1}: host name; target in the status card while connecting or switching.
    TargetLabel,

    // --- Best location (best.rs, look.rs, feature 013) ---
    BestName,
    BestHint,
    /// {0}: location chosen, for example "Germany, Berlin".
    BestChosen,
    NoticeBestAlready,
    NoticeBestUnknown,
    NoticeBestLoading,
    NoticeBestNoList,

    // --- Main button (look.rs) ---
    ButtonConnect,
    ButtonDisconnect,
    ButtonReconnecting,
    ButtonSwitching,
    ButtonConnecting,
    ButtonDisconnecting,
    ButtonBlocking,
    ButtonBlock,
    ButtonUnblock,

    // --- Public address (look.rs) ---
    ExternalIpAsking,
    ExternalIpFailed,

    // --- System tray icon (trayicon.rs) ---
    /// Hover text of the icon in the system tray. {0}: short state as at the bottom of the sidebar.
    TrayTooltip,
    // --- Tray menu (look.rs, feature 014) ---
    TrayShow,
    TrayBest,
    TrayFavorites,
    TrayNoFavorites,
    TrayFavoriteAdd,
    TrayFavoriteRemove,
    /// A favourite that is not in the list (any more), without its host name.
    TrayFavoriteUnavailable,
    /// A favourite while the server list is loading, without its host name.
    TrayFavoriteLoading,
    TrayLocations,
    ContinentEurope,
    ContinentAsia,
    ContinentNorthAmerica,
    ContinentSouthAmerica,
    ContinentAfrica,
    ContinentOceania,
    TrayLocationsLoading,
    TrayLocationsMissing,
    TrayStreaming,
    /// "{0} (Streaming)": a streaming favourite; {0} is the country.
    TrayStreamingPlace,
    TrayAutomatic,
    /// "{0} (free)": a free location in the menu, as the badge "free" in the list.
    TrayFreePlace,
    TrayAbout,
    TrayQuit,
    /// {0} is the new version, built by Celata from three numbers.
    UpdateAvailable,
    UpdateBlockedNotice,
    AboutVersion,

    // --- Notifications (notify.rs, feature 014) ---
    /// {0} is country and place, never a host name.
    NotifyConnected,
    NotifyDisconnected,
    NotifyLost,
    NotifyLostBlocked,
    NotifyFailed,
    NotifySwitched,
    NotifySwitchedBack,
    NotifyBlocked,
    NotifyUnblocked,
    NotifyActionFailed,

    // --- Settings of Celata, section "Application" (main.rs, feature 014) ---
    /// {0} is the path of the entry.
    SettingsAutostartForeign,
    SettingsAutostartFailed,

    // --- Command line (cli.rs, unblock.rs, feature 014) ---
    CliHelp,
    /// {0} is the argument, cleaned and cut.
    CliUnknown,
    CliFreed,
    CliNotBlocked,
    CliUnreadable,
    /// {0} is the reason.
    CliFailed,
    CliEmergency,
    StateServiceStartingTitle,
    StateServiceStartingText,
    StateServiceStartingBlockedTitle,
    StateServiceStartingBlockedText,
    StateServiceUnreachableAutoText,
    StartNeedsPasswordTitle,
    StartNeedsPasswordText,
    StartNotInstalledTitle,
    StartNotInstalledText,
    StartFailedTitle,
    StartFailedText,
    StartNoPromptNotice,
    SetupGuideBlockedNotice,
    // END OF KEYS (marker: add new keys before this line)
}

/// Text of a key in a language.
pub fn text(language: Language, key: Key) -> &'static str {
    match language {
        Language::En => en::text(key),
        Language::De => de::text(key),
    }
}

/// Country name for the country code (ISO 3166-1, upper case); `None` for an unknown code.
pub fn country(language: Language, code: &str) -> Option<&'static str> {
    match language {
        Language::En => en::country(code),
        Language::De => de::country(code),
    }
}

/// Place name for the name from the API. An addition such as " #2" is kept, unknown places stay the
/// same; for "state, city" every part is translated on its own.
pub fn place(language: Language, original: &str) -> String {
    original
        .split(", ")
        .map(|part| {
            let (base, suffix) = match part.rfind(" #") {
                Some(index) => part.split_at(index),
                None => (part, ""),
            };
            let name = match language {
                Language::En => en::place(base),
                Language::De => de::place(base),
            };
            format!("{}{suffix}", name.unwrap_or(base))
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Message without a fixed language; it is translated only when it is shown ([`Phrase::render`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phrase {
    /// Fixed text.
    Key(Key),
    /// Text with placeholders `{0}`, `{1}` …; every value is a phrase itself.
    Fill(Key, Vec<Phrase>),
    /// Text from outside that stays unchanged (error text of the service, server name, number).
    Raw(String),
    /// Country name in the respective language; an unknown code shows `fallback` (the name from the API).
    Country { code: String, fallback: String },
}

impl Phrase {
    pub fn key(key: Key) -> Self {
        Self::Key(key)
    }

    pub fn fill(key: Key, values: impl IntoIterator<Item = Phrase>) -> Self {
        Self::Fill(key, values.into_iter().collect())
    }

    pub fn raw(text: impl Into<String>) -> Self {
        Self::Raw(text.into())
    }

    pub fn country(code: impl Into<String>, fallback: impl Into<String>) -> Self {
        Self::Country {
            code: code.into(),
            fallback: fallback.into(),
        }
    }

    /// "{text} Reason: {reason}" in the respective language.
    pub fn with_reason(text: Phrase, reason: Phrase) -> Self {
        Self::fill(Key::WithReason, [text, reason])
    }

    /// Assembles the text in a language.
    pub fn render(&self, language: Language) -> String {
        match self {
            Self::Key(key) => text(language, *key).to_owned(),
            Self::Fill(key, values) => {
                let values: Vec<String> = values.iter().map(|value| value.render(language)).collect();
                fill(text(language, *key), &values)
            }
            Self::Raw(text) => text.clone(),
            Self::Country { code, fallback } => country(language, code).unwrap_or(fallback).to_owned(),
        }
    }
}

impl From<Key> for Phrase {
    fn from(key: Key) -> Self {
        Self::Key(key)
    }
}

/// For tests only: German text of a key or a message, so that the existing checks keep running with
/// their German expected values.
#[cfg(test)]
pub(crate) trait Deutsch {
    fn de(&self) -> String;
}

#[cfg(test)]
impl Deutsch for Key {
    fn de(&self) -> String {
        text(Language::De, *self).to_owned()
    }
}

#[cfg(test)]
impl Deutsch for Phrase {
    fn de(&self) -> String {
        self.render(Language::De)
    }
}

/// Replaces `{n}` with the n-th value. If the value is missing, the placeholder stays.
pub fn fill(template: &str, values: &[String]) -> String {
    let mut result = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        result.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let digits = after.bytes().take_while(u8::is_ascii_digit).count();
        let value = (digits > 0 && after[digits..].starts_with('}'))
            .then(|| after[..digits].parse::<usize>().ok())
            .flatten()
            .and_then(|index| values.get(index));
        match value {
            Some(value) => {
                result.push_str(value);
                rest = &after[digits + 1..];
            }
            None => {
                result.push('{');
                rest = after;
            }
        }
    }
    result.push_str(rest);
    result
}

/// Numbers of the placeholders `{n}` in a text, sorted and without duplicates.
pub fn placeholders(template: &str) -> Vec<usize> {
    let mut found = Vec::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        let digits = after.bytes().take_while(u8::is_ascii_digit).count();
        if digits > 0
            && after[digits..].starts_with('}')
            && let Ok(index) = after[..digits].parse()
        {
            found.push(index);
        }
        rest = after;
    }
    found.sort_unstable();
    found.dedup();
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::names;

    #[test]
    fn kennungen() {
        assert_eq!(Language::from_code("de"), Some(Language::De));
        assert_eq!(Language::from_code("DE"), Some(Language::De));
        assert_eq!(Language::from_code(" de "), Some(Language::De));
        assert_eq!(Language::from_code("en"), Some(Language::En));
        assert_eq!(Language::from_code("xx"), None);
        assert_eq!(Language::from_code(""), None);
        assert_eq!(Language::default(), Language::En);
        for language in Language::ALL {
            assert_eq!(Language::from_code(language.code()), Some(language));
        }
    }

    #[test]
    fn sprachnamen_in_eigener_schreibweise() {
        assert_eq!(Language::En.native_name(), "English");
        assert_eq!(Language::De.native_name(), "Deutsch");
    }

    #[test]
    fn auswahl_einer_sprache() {
        assert_eq!(choose(Language::En, "de"), Some(Language::De));
        assert_eq!(choose(Language::De, "en"), Some(Language::En));
        assert_eq!(choose(Language::De, "de"), None);
        assert_eq!(choose(Language::De, " DE "), None);
        assert_eq!(choose(Language::En, "xx"), None);
        assert_eq!(choose(Language::En, ""), None);
    }

    #[test]
    fn platzhalter_werden_ersetzt() {
        let values = ["a".to_owned(), "b".to_owned()];
        assert_eq!(fill("{1} vor {0}", &values), "b vor a");
        assert_eq!(fill("{2} fehlt", &values), "{2} fehlt");
        assert_eq!(fill("{x} und {} bleiben", &values), "{x} und {} bleiben");
        assert_eq!(fill("ohne", &values), "ohne");
        assert_eq!(placeholders("{1} und {0} und {1}"), vec![0, 1]);
        assert_eq!(placeholders("{} {x}"), Vec::<usize>::new());
    }

    #[test]
    fn phrasen_werden_spaet_uebersetzt() {
        let phrase = Phrase::with_reason(Phrase::key(Key::LanguageSaveFailed), Phrase::raw("{0} kaputt"));
        let de = phrase.render(Language::De);
        let en = phrase.render(Language::En);
        assert!(de.ends_with("Grund: {0} kaputt"), "{de}");
        assert!(en.ends_with("Reason: {0} kaputt"), "{en}");
        assert_ne!(de, en);
        assert_eq!(Phrase::raw("{0}").render(Language::De), "{0}");
        let land = Phrase::country("DE", "Germany");
        assert_eq!(land.render(Language::De), "Deutschland");
        assert_eq!(land.render(Language::En), "Germany");
        assert_eq!(Phrase::country("XX", "Atlantis").render(Language::De), "Atlantis");
    }

    #[test]
    fn jeder_text_vorhanden_mit_gleichen_platzhaltern() {
        for key in Key::ALL {
            let reference = placeholders(text(Language::En, *key));
            for language in Language::ALL {
                let text = text(language, *key);
                assert!(!text.trim().is_empty(), "{key:?} ist auf {} leer", language.code());
                assert_eq!(
                    placeholders(text),
                    reference,
                    "{key:?}: Platzhalter auf {} anders",
                    language.code()
                );
            }
        }
    }

    /// Narrow places with texts from Rust.
    #[test]
    fn laengengrenzen_enger_stellen() {
        let limits = [
            (Key::ButtonConnect, 24),
            (Key::ButtonDisconnect, 24),
            (Key::ButtonReconnecting, 24),
            (Key::ButtonSwitching, 24),
            (Key::ButtonConnecting, 24),
            (Key::ButtonDisconnecting, 24),
            (Key::ButtonBlocking, 24),
            (Key::ButtonBlock, 24),
            (Key::ButtonUnblock, 24),
            (Key::CloseKeepConnected, 32),
            (Key::CloseKeepBlocked, 32),
            (Key::CloseAnyway, 32),
            (Key::ConfirmRebuildOk, 32),
            (Key::ConfirmSettingOk, 32),
            (Key::ConfirmResetOk, 32),
        ];
        for (key, limit) in limits {
            for language in Language::ALL {
                let text = text(language, key);
                assert!(
                    text.chars().count() <= limit,
                    "{key:?} auf {}: {text:?} länger als {limit}",
                    language.code()
                );
            }
        }
    }

    #[test]
    fn jedes_land_hat_in_jeder_sprache_einen_namen() {
        for code in names::CODES {
            for language in Language::ALL {
                assert!(
                    country(language, code).is_some(),
                    "{code} fehlt auf {}",
                    language.code()
                );
            }
        }
        assert_eq!(country(Language::De, "XX"), None);
    }

    #[test]
    fn laender_in_beiden_sprachen() {
        assert_eq!(country(Language::De, "DE"), Some("Deutschland"));
        assert_eq!(country(Language::De, "AT"), Some("Österreich"));
        assert_eq!(country(Language::En, "DE"), Some("Germany"));
        assert_eq!(country(Language::En, "GB"), Some("United Kingdom"));
    }

    #[test]
    fn orte_auf_deutsch_mit_zusatz() {
        let place = |name| place(Language::De, name);
        assert_eq!(place("Vienna"), "Wien");
        assert_eq!(place("Zurich"), "Zürich");
        assert_eq!(place("Helsinki #1"), "Helsinki #1");
        assert_eq!(place("Milan #2"), "Mailand #2");
        assert_eq!(place("Texas, Dallas"), "Texas, Dallas");
        assert_eq!(place("California, Los Angeles"), "Kalifornien, Los Angeles");
        assert_eq!(place("Washington, D.C."), "Washington, D.C.");
    }

    #[test]
    fn orte_auf_englisch_unveraendert() {
        for name in ["Vienna", "Milan #2", "California, Los Angeles"] {
            assert_eq!(place(Language::En, name), name);
        }
    }

    #[test]
    fn jede_sprache_hat_eine_uebersetzung_der_oberflaeche() {
        let root = env!("CARGO_MANIFEST_DIR");
        for language in Language::ALL.into_iter().filter(|language| *language != Language::En) {
            let path = format!("{root}/ui/translations/{}/LC_MESSAGES/celata-ui.po", language.code());
            assert!(std::path::Path::new(&path).is_file(), "{path} fehlt");
        }
    }
}
