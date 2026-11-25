// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! English texts. Every key needs a text here, otherwise the program does not build.
//!
//! English is the fallback language. Terms follow the project glossary (kill switch, block internet,
//! unblock internet, internet blocked); warnings carry every part of the German sentence.

use super::Key;

#[expect(
    clippy::match_same_arms,
    reason = "every key keeps its own text, even where two texts are equal today"
)]
pub(super) fn text(key: Key) -> &'static str {
    match key {
        // --- General ---
        Key::WithReason => "{0} Reason: {1}",
        Key::TerminalPrefix => "Celata: {0}",

        // --- Numbers and durations ---
        Key::DurationUnderMinute => "less than 1 min",
        Key::DurationMinutes => "{0} min",
        Key::DurationHoursMinutes => "{0} h {1} min",

        // --- Donation hint ---
        Key::DonationCardTitle => "Enjoying Celata?",
        Key::DonationCardText => "It's free. A small donation helps further development.",
        Key::DonationButton => "Donate via Ko-fi",
        Key::DonationButtonLabel => "Donate via Ko-fi, opens the browser",
        Key::DonationSectionTitle => "Support Celata",
        Key::DonationSectionText => {
            "Celata is free. If you would like to support its further development, I would appreciate a small \
             donation."
        }
        Key::DonationToggle => "Show donation hint",
        Key::DonationToggleDescription => "Shows a donation button at the bottom of the sidebar.",
        Key::DonationBlockedNotice => {
            "The internet is blocked. The page will load once the VPN is connected or the internet is unblocked."
        }
        Key::DonationOpenFailed => {
            "The browser could not be opened. You can copy the link and open it in your browser yourself."
        }

        // --- Settings of Celata ---
        Key::LanguageSaveFailed => "The language choice could not be saved. It applies until Celata is closed.",
        // --- Terminal (main.rs) ---
        Key::TerminalLedgerSaveFailed => "The record of applied settings could not be saved.",
        Key::TerminalThreadFailed => "A background thread ({0}) could not be started.",
        Key::TerminalTranslationMissing => {
            "The built-in translation for “{0}” is missing. Celata continues in English."
        }
        Key::TerminalAppIdFailed => "The program ID for the desktop could not be set ({0}).",
        Key::DesktopEntryComment => "Graphical interface for the hide.me VPN client",
        Key::TerminalDesktopEntryFailed => {
            "The desktop entry or the program icon could not be created ({0}). Celata continues normally."
        }
        Key::NoGraphicalSession => "Celata needs a graphical session (Wayland, or X11 untested).",

        // --- Questions (main.rs) ---
        Key::ConfirmRebuildTitle => "Rebuild the internet block?",
        Key::ConfirmRebuildText => {
            "Changed settings require the blocking routes to be rebuilt. During this, the internet is briefly \
             unprotected, even with the kill switch. Celata then connects."
        }
        Key::ConfirmRebuildOk => "Connect anyway",
        Key::ConfirmSettingTitle => "Apply setting?",
        Key::ConfirmSettingOk => "Apply",
        Key::ConfirmResetTitle => "Reset to defaults?",
        Key::ConfirmResetOk => "Reset",
        Key::ResetAdvancedText => "All values under “Advanced” return to the hide.me defaults.",
        Key::ResetAllWithFiltersText => {
            "All settings return to the hide.me defaults. Filters, exceptions and custom values are deleted."
        }
        Key::ResetAllText => "All settings return to the hide.me defaults. Custom values are deleted.",

        // --- Settings page (main.rs) ---
        Key::SettingsSaveFailed => "The settings could not be saved: {0}",
        Key::SettingsApplying => "Applying …",
        Key::DohDefault => "{0} servers from Quad9 (hide.me default).",
        Key::DohNone => "No servers: Celata only uses the fallback DNS servers.",
        Key::DohCustom => "{0} custom servers.",

        // --- Server list (servers.rs, main.rs) ---
        Key::ListStale => "The list could not be updated. An older version is shown.",
        Key::ServerListNetwork => "The server list could not be loaded. Check your internet connection and try again.",
        Key::ServerListInvalid => "The server list from hide.me is unreadable. Try again later.",

        // --- Categories (categories.rs) ---
        Key::CategoriesNetwork => "The categories could not be loaded. Check the internet connection.",
        Key::CategoriesInvalid => "The category list from hide.me is unreadable.",
        // --- Errors of the service (daemon/http.rs) ---
        Key::ErrorNotRunning => "The service is not reachable.",
        Key::ErrorPermissionDenied => "No permission for the service.",
        Key::ErrorBusy => "The service is busy at the moment.",
        Key::ErrorTimeout => "The service did not respond in time.",
        Key::ErrorConnectionLost => "The connection to the service was interrupted.",
        Key::ErrorStatus => "The service reports HTTP error {0}.",
        Key::ErrorInvalidResponse => "The response of the service is unreadable.",
        Key::ErrorResponseTooLarge => "The response of the service is too large.",
        Key::ErrorService => "The service reports: {0}",
        Key::ErrorInvalidHost => "The server name is invalid.",
        // --- Streaming (streaming.rs) ---
        Key::StreamingName => "Streaming {0}",
        Key::StreamingLabel => "{0} ({1})",
        // --- Field errors (settings.rs) ---
        Key::FieldNameError => "Only letters, digits, dot, hyphen, underscore and * are allowed.",
        Key::FieldNetworkError => "Enter a network in the form 192.168.1.0/24.",
        Key::FieldDnsError => "IP address, optionally with port, e.g. 9.9.9.9:53.",
        Key::FieldStampError => "Entry must be a DNS stamp (sdns://…).",
        Key::FieldBackupError => "Only a file name; the backup is always stored in /run/hide.me.",
        Key::FieldInterfaceError => "1 to 15 characters from letters, digits, dot, hyphen and underscore.",
        Key::FieldInterfaceTaken => "Another network interface already has this name.",
        Key::FieldTableReserved => "Tables 253 to 255 belong to the system.",
        Key::FieldRange => "Number between {0} and {1}.",
        Key::FieldPgError => "Only off, 12+, 18+ or 21+.",
        Key::FieldRiskUnknown => "Unknown risk level.",
        Key::FieldIllegalUnknown => "Unknown type of illegal content.",
        Key::FieldCategoryUnknown => "Unknown category.",
        Key::FieldNameInBothLists => "A name must not be in both lists.",
        Key::FieldDnsRequired => "At least one DNS server is required.",
        Key::FieldIpVersionUnknown => "Unknown IP version.",
        Key::FieldNotSelectable => "This entry cannot be selected.",
        Key::FieldUnknownSetting => "Unknown setting “{0}”.",

        // --- State and file of the settings (settings.rs) ---
        Key::SettingsVersionNotice => {
            "These settings are only tested with hide.me {0}. Reset them to defaults or update Celata."
        }
        Key::UntestedVersion => "{0} The service reports version {1}.",
        Key::SettingsAppliesNow => "Applies to the current connection.",
        Key::SettingsAppliesNext => "Applies from the next connection.",
        Key::SettingsFileBroken => {
            "The saved settings could not be read. The defaults apply; the file stays unchanged until the next \
             change."
        }
        Key::SettingsFileNewer => {
            "The saved settings come from a newer version of Celata. The defaults apply; the file stays \
             unchanged until the next change."
        }

        // --- Warnings before applying (settings.rs) ---
        Key::WarningRebuild => {
            "While applying, the internet is briefly unprotected, even with the kill switch. If the new \
             connection fails, this happens a second time when switching back."
        }
        Key::WarningKillSwitch => "Without the kill switch, your data flows unprotected if the VPN fails.",
        Key::WarningPortForward => "Devices on the internet can then open connections to this computer.",
        Key::WarningSplit => "Traffic to these networks does not go through the VPN.",
        Key::WarningWideSplit => "This makes almost all traffic bypass the VPN.",
        Key::WarningResolver => "These servers see which hide.me server you connect to.",
        Key::WarningDohOff => {
            "Without DNS over HTTPS, fallback DNS servers and your network see which hide.me server you connect \
             to."
        }
        Key::WarningAdvanced => "Wrong values can disrupt the network and DNS of this computer.",
        Key::WarningResolvBackup => "The resolv.conf backup can overwrite earlier DNS settings if it is not restored.",
        Key::SentencePair => "{0} {1}",
        Key::ParagraphPair => "{0}\n\n{1}",

        // --- Hints in the overview (settings.rs) ---
        Key::HintKillSwitchOff => "The kill switch is off. If the VPN fails, data flows unprotected.",
        Key::HintSplitTunnel => "Split tunneling active: {0}",
        Key::HintPortForward => "Port forwarding is requested.",
        Key::HintDohOff => "DNS over HTTPS is off: the VPN server is looked up via plain DNS requests.",
        Key::HintPlainDns => "Custom fallback DNS servers: {0}",
        Key::HintAdvanced => "Values under “Advanced” differ from the defaults.",
        Key::HintDnsLeak => {
            "DNS leak risk: this system resolves names through systemd-resolved, so DNS requests can bypass the VPN. See “DNS leaks” in the Celata documentation."
        }
        // --- States (state.rs) ---
        Key::StateCheckingTitle => "Checking status …",
        Key::StateCheckingText => "Celata is querying the hide.me service.",
        Key::StateNotConnectedTitle => "Not connected",
        Key::StateNotConnectedText => "Your internet works normally, but without VPN. Choose a server to connect.",
        Key::StateConnectingTitle => "Connecting …",
        Key::StateConnectingText => "The connection is being established. Please wait a moment.",
        Key::StateConnectingBlockedTitle => "Connecting, internet blocked",
        Key::StateConnectingBlockedText => {
            "The service is restoring the connection. Until then, the kill switch is blocking your internet for \
             your protection. Wait a moment or unblock the internet. To unblock, click the red area on the right \
             of the button below."
        }
        Key::StateConnectingToTitle => "Connecting to {0} …",
        Key::StateConnectingBlockedToTitle => "Connecting to {0}, internet blocked",
        Key::StateConnectedTitle => "Connected",
        Key::StateConnectedText => "Your traffic runs protected through the VPN server.",
        Key::StateDisconnectingTitle => "Disconnecting …",
        Key::StateDisconnectingText => "The connection is being closed. Please wait a moment.",
        Key::StateSwitchingTitle => "Switching to {0} …",
        Key::StateSwitchingText => {
            "Celata is connecting you to the new server. Until then, your internet is blocked for your \
             protection."
        }
        Key::StateReturningTitle => "Returning to {0} …",
        Key::StateReturningText => {
            "The chosen server was not reachable. Celata is connecting you to the previous one again."
        }
        Key::StateReapplyingTitle => "Applying settings …",
        Key::StateReapplyingText => "Celata is reconnecting so that the changed settings apply. Please wait a moment.",
        Key::StateReapplyingBackTitle => "Restoring previous settings …",
        Key::StateReapplyingBackText => {
            "No connection could be established with the new settings. Celata is connecting with the previous \
             ones."
        }
        Key::StateBlockedTitle => "Internet blocked",
        Key::StateBlockedText => {
            "There is no VPN connection, and the kill switch is blocking your internet for your protection. \
             Reconnect or unblock the internet. To unblock, click the red area on the right of the button below."
        }
        Key::StateServiceUnreachableTitle => "Service unavailable",
        Key::StateServiceUnreachableText => {
            "The hide.me service is not running. Start it in the terminal with this command. Celata detects the \
             start by itself:"
        }
        Key::StateServiceUnreachableBlockedTitle => "Service unavailable, internet blocked",
        Key::StateServiceUnreachableBlockedText => {
            "The service is not responding, but the block is still active. Stop the service in the terminal with \
             this command, then your internet is free again:"
        }
        Key::StateNoPermissionTitle => "No permission",
        Key::StateNoPermissionText => {
            "Your user may not control the service: it is not in the “celata” group, or you have not logged in \
             again since it was added. Add it with this command, then log out and back in once:"
        }
        Key::StateServiceFaultyTitle => "Service not responding correctly",
        Key::StateServiceFaultyText => {
            "The hide.me service is running but not responding as expected. Celata retries every second. If the \
             message remains, restart the service in the terminal:"
        }
        Key::DetailReason => "Reason: {0}",
        Key::VersionWarning => "The service reports version {0}; Celata is tested with {1}. Some displays may differ.",

        // --- Question on closing (state.rs) ---
        Key::CloseConnectedText => "The VPN is connected. Should the connection be closed when closing Celata?",
        Key::CloseBusyText => {
            "The connection is being established or closed right now. If establishing it fails after closing, the \
             internet may stay blocked; Celata shows this the next time it opens."
        }
        Key::CloseBlockedText => {
            "Your internet is blocked. If you close Celata now without disconnecting, the block stays in place. \
             “Disconnect and close” removes it; afterwards your data flows without VPN."
        }
        Key::CloseOfflineBlockedText => {
            "Your internet is blocked, and the service is not responding. After closing, remove the block in the \
             terminal with this command:"
        }
        Key::CloseKeepConnected => "Stay connected and close",
        Key::CloseKeepBlocked => "Keep blocked and close",
        Key::CloseAnyway => "Close anyway",

        // --- Notices after actions (state.rs) ---
        Key::NoticeConnectNotStarted => "The connection was not started. Try again in a moment.",
        Key::NoticeConnectRejected => {
            "The service rejected the settings, so no connection was made. Check the settings. Reason: {0}"
        }
        Key::NoticeConnectFailedReleased => {
            "The connection could not be established. Your internet is free again, but no VPN is active. Choose a \
             server and try again."
        }
        Key::NoticeConnectFailedBlocked => {
            "The connection could not be established. The block stays in place for your protection. Try another \
             server or unblock the internet."
        }
        Key::NoticeConnectReleaseFailed => {
            "The connection could not be established, and the internet could not be released again. Stop the \
             service in the terminal with this command:"
        }
        Key::NoticeRouteRejected => "The internet was not blocked: the service rejected the settings ({0}).",
        Key::NoticeRouteFailed => "The internet could not be blocked.",
        Key::RouteHint => "This requires the kill switch to be turned on in the settings.",
        Key::NoticeUnblocked => "The block has been removed. Your internet now runs without VPN.",
        Key::NoticeUnblockFailed => {
            "The block could not be removed. Stop the service in the terminal with this command; this removes the \
             block safely:"
        }
        Key::NoticeCloseDisconnectFailed => {
            "“Disconnect and close” failed, so Celata stays open. Stop the service in the terminal with this \
             command, then the connection is closed and the internet is free:"
        }
        Key::NoticeDisconnectFailed => {
            "The connection could not be closed. Stop the service in the terminal with this command, then the \
             connection is closed and the internet is free:"
        }
        Key::UnblockWarning => {
            "If you unblock the internet, no VPN is active: your data flows unprotected over your normal internet \
             connection until you connect again. Unblock the internet anyway?"
        }

        // --- Reconnecting after a change of settings (state.rs) ---
        Key::NoticeReapplied => "Connection re-established with the new settings.",
        Key::NoticeReapplyLater => "They apply from the next connection.",
        Key::NoticeReapplyReturned => "New settings not applied; connection restored with the previous settings: {0}",
        Key::NoticeReapplyRejected => {
            "New settings not applied: the service rejected them ({0}). The connection is unchanged. Check the \
             settings."
        }
        Key::NoticeReapplyRefused => {
            "New settings not applied: the kill switch is not ready or cannot be checked; without it, your \
             internet would briefly be without VPN while reconnecting. {0}"
        }
        Key::NoticeReapplyUnreachable => "New settings not applied: the server is not reachable right now. {0}",
        Key::NoticeReapplyNotStarted => "New settings not applied. {0}",
        Key::NoticeReapplyStranded => {
            "Reconnecting failed, even with the previous settings. Choose a server or unblock the internet."
        }
        Key::NoticeReapplyUnclear => "Reconnecting could not be started.",
        // --- Switching servers (switch.rs) ---
        Key::NoticeSwitchReturned => {
            "“{0}” was not reachable. You are connected to “{1}” again. Choose another server if you want to \
             switch."
        }
        Key::NoticeSwitchStranded => {
            "The switch failed; “{0}” was not reachable either. Choose another server or unblock the internet."
        }
        Key::NoticeSwitchRefused => {
            "Switching is not possible: the kill switch of the service is not active or cannot be checked. \
             Without it, your internet would briefly be without VPN during the switch. Disconnect first if you \
             accept that."
        }
        Key::NoticeSwitchUnreachable => {
            "“{0}” is not reachable right now. You stay connected to “{1}”. Try again later or choose another \
             server."
        }
        Key::NoticeSwitchUnclear => "The switch could not be started.",
        Key::NoticeSwitchRejected => "The service rejected the settings: {0}",
        // --- Server list and locations (servers.rs, look.rs) ---
        Key::ServerAutomatic => "Automatic",
        Key::ServerFree => "Free server",
        Key::ServerPlain => "Server",
        Key::AutomaticPlace => "Automatic, hide.me chooses the location",
        Key::StreamingPlace => "Streaming",
        Key::LocationChoice => "{0}, {1}",
        Key::ListHeadingFavorites => "Favorites",
        Key::ListHeadingAll => "All locations",
        Key::TargetLabel => "{0} ({1})",
        // --- Best location (best.rs, look.rs) ---
        Key::BestName => "Best location",
        Key::BestHint => "Automatic, Celata chooses the location",
        Key::BestChosen => "Chosen: {0}",
        Key::NoticeBestAlready => "Already connected to the best location.",
        Key::NoticeBestUnknown => "The best location cannot be determined. Please choose a location.",
        Key::NoticeBestLoading => "The locations are still loading. Please try again in a moment.",
        Key::NoticeBestNoList => "No locations are loaded. Please load the list again or choose a location later.",

        // --- Main button (look.rs) ---
        Key::ButtonConnect => "Connect VPN",
        Key::ButtonDisconnect => "Disconnect",
        Key::ButtonReconnecting => "Reconnecting …",
        Key::ButtonSwitching => "Switching …",
        Key::ButtonConnecting => "Connecting …",
        Key::ButtonDisconnecting => "Disconnecting …",
        Key::ButtonBlocking => "Blocking internet …",
        Key::ButtonBlock => "Block internet",
        Key::ButtonUnblock => "Unblock internet",

        // --- Public address (look.rs) ---
        Key::ExternalIpAsking => "determining …",
        Key::ExternalIpFailed => "not available",
        Key::TrayTooltip => "Celata – {0}",
        // --- Tray menu (look.rs, feature 014) ---
        Key::TrayShow => "Show Celata",
        Key::TrayBest => "Connect to best location",
        Key::TrayFavorites => "Favorites",
        Key::TrayNoFavorites => "No favorites",
        Key::TrayFavoriteAdd => "Add current location to favorites",
        Key::TrayFavoriteRemove => "Remove current location from favorites",
        Key::TrayFavoriteUnavailable => "Location not available",
        Key::TrayFavoriteLoading => "Loading …",
        Key::TrayLocations => "Locations",
        Key::ContinentEurope => "Europe",
        Key::ContinentAsia => "Asia",
        Key::ContinentNorthAmerica => "North America",
        Key::ContinentSouthAmerica => "South America",
        Key::ContinentAfrica => "Africa",
        Key::ContinentOceania => "Oceania",
        Key::TrayLocationsLoading => "Loading locations …",
        Key::TrayLocationsMissing => "Locations not available",
        Key::TrayStreaming => "Streaming",
        Key::TrayStreamingPlace => "{0} (Streaming)",
        Key::TrayAutomatic => "Automatic",
        Key::TrayFreePlace => "{0} (free)",
        Key::TrayAbout => "About Celata",
        Key::TrayQuit => "Quit",
        Key::UpdateAvailable => "Celata {0} is available",
        Key::UpdateBlockedNotice => {
            "The internet is blocked. The page will load once the VPN is connected or the internet is unblocked."
        }
        Key::AboutVersion => "Version {0}",

        // --- Notifications (notify.rs, feature 014) ---
        Key::NotifyConnected => "Connected: {0}",
        Key::NotifyDisconnected => "Disconnected",
        Key::NotifyLost => "Connection lost",
        Key::NotifyLostBlocked => "Connection lost, internet blocked",
        Key::NotifyFailed => "Could not connect to {0}",
        Key::NotifySwitched => "Switched to {0}",
        Key::NotifySwitchedBack => "Switch failed, back to {0}",
        Key::NotifyBlocked => "Internet blocked",
        Key::NotifyUnblocked => "Internet unblocked",
        Key::NotifyActionFailed => "Action failed. Details in the Celata window.",

        // --- Settings of Celata, section "Application" (main.rs, feature 014) ---
        Key::SettingsAutostartForeign => "Another entry starts Celata at login ({0}).",
        Key::SettingsAutostartFailed => "Could not change the login start entry.",

        // --- Command line (cli.rs, unblock.rs, feature 014) ---
        Key::CliHelp => {
            "Usage: celata [OPTION]\n\nWithout an option, Celata opens its window.\n\n  --version      Print the version.\n  --unblock      Unblock the internet without opening the window.\n  --polkit-rule  Print the rule that lets members of the group celata start the service.\n  --help         Show this help."
        }
        Key::CliUnknown => "Unknown option: {0}. See celata --help.",
        Key::CliFreed => "Internet unblocked.",
        Key::CliNotBlocked => "The internet is not blocked.",
        Key::CliUnreadable => "The routing table could not be read, so Celata changed nothing.",
        Key::CliFailed => "The block could not be lifted. {0}",
        Key::CliEmergency => "Emergency exit: sudo systemctl stop celata-hide.me.service",
        Key::StateServiceStartingTitle => "Starting service …",
        Key::StateServiceStartingText => {
            "Celata is starting the hide.me service. After logging in this can take a few seconds, until the network is \
             ready."
        }
        Key::StateServiceStartingBlockedTitle => "Starting service, internet blocked",
        Key::StateServiceStartingBlockedText => {
            "Celata is starting the hide.me service. The kill switch still blocks your internet; Celata does not lift \
             the block by itself."
        }
        Key::StateServiceUnreachableAutoText => {
            "The hide.me service is not running. Celata starts it as soon as you connect."
        }
        Key::StartNeedsPasswordTitle => "Service could not be started",
        Key::StartNeedsPasswordText => {
            "Celata may not start the hide.me service without a password on this computer. Click “Start service”; the \
             system then asks for the administrator password."
        }
        Key::StartNotInstalledTitle => "Service not set up",
        Key::StartNotInstalledText => {
            "The hide.me service for Celata is not set up on this computer. Install the Celata package or follow the \
             setup guide."
        }
        Key::StartFailedText => {
            "The hide.me service was started but stopped again right away. Common reasons: the hide.me client is \
             missing or has no Access-Token yet. This command shows the last messages of the service:"
        }
        Key::StartNoPromptNotice => "The system could not ask for the password. Start the service in a terminal:",
        Key::SetupGuideBlockedNotice => {
            "The internet is blocked. The setup guide opens once the block is lifted or a VPN connection exists."
        }
        Key::StartFailedTitle => "Service does not start",
        // END OF KEYS (marker: add new texts before this line)
    }
}

/// English country names (ISO 3166-1, upper case), in their common short forms.
pub(super) fn country(code: &str) -> Option<&'static str> {
    Some(match code {
        "AE" => "United Arab Emirates",
        "AL" => "Albania",
        "AM" => "Armenia",
        "AR" => "Argentina",
        "AT" => "Austria",
        "AU" => "Australia",
        "AZ" => "Azerbaijan",
        "BA" => "Bosnia and Herzegovina",
        "BD" => "Bangladesh",
        "BE" => "Belgium",
        "BG" => "Bulgaria",
        "BO" => "Bolivia",
        "BR" => "Brazil",
        "BY" => "Belarus",
        "CA" => "Canada",
        "CH" => "Switzerland",
        "CL" => "Chile",
        "CN" => "China",
        "CO" => "Colombia",
        "CR" => "Costa Rica",
        "CY" => "Cyprus",
        "CZ" => "Czechia",
        "DE" => "Germany",
        "DK" => "Denmark",
        "EC" => "Ecuador",
        "EE" => "Estonia",
        "EG" => "Egypt",
        "ES" => "Spain",
        "FI" => "Finland",
        "FR" => "France",
        "GB" => "United Kingdom",
        "GE" => "Georgia",
        "GR" => "Greece",
        "HK" => "Hong Kong",
        "HR" => "Croatia",
        "HU" => "Hungary",
        "ID" => "Indonesia",
        "IE" => "Ireland",
        "IL" => "Israel",
        "IN" => "India",
        "IS" => "Iceland",
        "IT" => "Italy",
        "JP" => "Japan",
        "KE" => "Kenya",
        "KH" => "Cambodia",
        "KR" => "South Korea",
        "KZ" => "Kazakhstan",
        "LK" => "Sri Lanka",
        "LT" => "Lithuania",
        "LU" => "Luxembourg",
        "LV" => "Latvia",
        "MA" => "Morocco",
        "MD" => "Moldova",
        "ME" => "Montenegro",
        "MK" => "North Macedonia",
        "MT" => "Malta",
        "MX" => "Mexico",
        "MY" => "Malaysia",
        "NG" => "Nigeria",
        "NL" => "Netherlands",
        "NO" => "Norway",
        "NP" => "Nepal",
        "NZ" => "New Zealand",
        "PA" => "Panama",
        "PE" => "Peru",
        "PH" => "Philippines",
        "PK" => "Pakistan",
        "PL" => "Poland",
        "PT" => "Portugal",
        "QA" => "Qatar",
        "RO" => "Romania",
        "RS" => "Serbia",
        "SA" => "Saudi Arabia",
        "SE" => "Sweden",
        "SG" => "Singapore",
        "SI" => "Slovenia",
        "SK" => "Slovakia",
        "TH" => "Thailand",
        "TR" => "Turkey",
        "TW" => "Taiwan",
        "UA" => "Ukraine",
        "US" => "United States",
        "UY" => "Uruguay",
        "VN" => "Vietnam",
        "ZA" => "South Africa",
        _ => return None,
    })
}

/// English place names: the name from the API stays unchanged.
pub(super) fn place(_original: &str) -> Option<&'static str> {
    None
}
