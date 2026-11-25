// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! German texts. Every key needs a text here, otherwise the program does not build.
//!
//! The texts are literally the German texts Celata had before it became bilingual. Terms follow the
//! project glossary.

use super::Key;

#[expect(
    clippy::match_same_arms,
    reason = "every key keeps its own text, even where two texts are equal today"
)]
pub(super) fn text(key: Key) -> &'static str {
    match key {
        // --- General ---
        Key::WithReason => "{0} Grund: {1}",
        Key::TerminalPrefix => "Celata: {0}",

        // --- Numbers and durations ---
        Key::DurationUnderMinute => "unter 1 Min.",
        Key::DurationMinutes => "{0} Min.",
        Key::DurationHoursMinutes => "{0} Std. {1} Min.",

        // --- Donation hint ---
        Key::DonationCardTitle => "Gefällt dir Celata?",
        Key::DonationCardText => "Es ist kostenlos. Eine kleine Spende hilft bei der Weiterentwicklung.",
        Key::DonationButton => "Spenden über Ko-fi",
        Key::DonationButtonLabel => "Spenden über Ko-fi, öffnet den Browser",
        Key::DonationSectionTitle => "Celata unterstützen",
        Key::DonationSectionText => {
            "Celata ist kostenlos. Wenn du die Weiterentwicklung unterstützen möchtest, freue ich mich über eine \
             kleine Spende."
        }
        Key::DonationToggle => "Spendenhinweis anzeigen",
        Key::DonationToggleDescription => "Zeigt unten in der Seitenleiste einen Knopf zum Spenden.",
        Key::DonationBlockedNotice => {
            "Das Internet ist gesperrt. Die Seite lädt, sobald das VPN verbunden oder die Sperre aufgehoben ist."
        }
        Key::DonationOpenFailed => {
            "Der Browser ließ sich nicht öffnen. Du kannst den Link kopieren und selbst im Browser öffnen."
        }

        // --- Settings of Celata ---
        Key::LanguageSaveFailed => "Die Sprachwahl ließ sich nicht speichern. Sie gilt, bis Celata geschlossen wird.",
        // --- Terminal (main.rs) ---
        Key::TerminalLedgerSaveFailed => "Der Übernahmestand ließ sich nicht speichern.",
        Key::TerminalThreadFailed => "Ein Hintergrund-Thread ({0}) konnte nicht gestartet werden.",
        Key::TerminalTranslationMissing => {
            "Die eingebaute Übersetzung für „{0}“ fehlt. Celata zeigt alles auf Englisch."
        }
        Key::TerminalAppIdFailed => "Die Programmkennung für den Desktop ließ sich nicht setzen ({0}).",
        Key::DesktopEntryComment => "Grafische Oberfläche für den hide.me-VPN-Client",
        Key::TerminalDesktopEntryFailed => {
            "Programmeintrag oder Programmsymbol ließen sich nicht anlegen ({0}). Celata läuft normal weiter."
        }
        Key::NoGraphicalSession => "Celata braucht eine grafische Sitzung (Wayland, oder X11 ungetestet).",

        // --- Questions (main.rs) ---
        Key::ConfirmRebuildTitle => "Sperre neu aufbauen?",
        Key::ConfirmRebuildText => {
            "Geänderte Einstellungen verlangen, dass die Sperr-Routen neu aufgebaut werden. Dabei ist das \
             Internet kurz ungeschützt, auch mit Internet-Sperre. Danach verbindet Celata."
        }
        Key::ConfirmRebuildOk => "Trotzdem verbinden",
        Key::ConfirmSettingTitle => "Einstellung übernehmen?",
        Key::ConfirmSettingOk => "Übernehmen",
        Key::ConfirmResetTitle => "Auf Standard zurücksetzen?",
        Key::ConfirmResetOk => "Zurücksetzen",
        Key::ResetAdvancedText => "Alle Werte unter „Erweitert“ gehen auf die Werte von hide.me zurück.",
        Key::ResetAllWithFiltersText => {
            "Alle Einstellungen gehen auf die Werte von hide.me zurück. Filter, Ausnahmen und eigene Werte werden \
             gelöscht."
        }
        Key::ResetAllText => "Alle Einstellungen gehen auf die Werte von hide.me zurück. Eigene Werte werden gelöscht.",

        // --- Settings page (main.rs) ---
        Key::SettingsSaveFailed => "Die Einstellungen ließen sich nicht speichern: {0}",
        Key::SettingsApplying => "Wird übernommen …",
        Key::DohDefault => "{0} Server von Quad9 (Standard von hide.me).",
        Key::DohNone => "Keine Server: Celata nutzt nur die Rückfall-DNS-Server.",
        Key::DohCustom => "{0} eigene Server.",

        // --- Server list (servers.rs, main.rs) ---
        Key::ListStale => "Die Liste ließ sich nicht aktualisieren. Angezeigt wird eine ältere Fassung.",
        Key::ServerListNetwork => {
            "Die Serverliste konnte nicht geladen werden. Prüfe deine Internetverbindung und versuche es erneut."
        }
        Key::ServerListInvalid => "Die Serverliste von hide.me ist unlesbar. Versuche es später erneut.",

        // --- Categories (categories.rs) ---
        Key::CategoriesNetwork => "Die Kategorien ließen sich nicht laden. Prüfe die Internetverbindung.",
        Key::CategoriesInvalid => "Die Kategorienliste von hide.me ist unlesbar.",
        // --- Errors of the service (daemon/http.rs) ---
        Key::ErrorNotRunning => "Der Dienst ist nicht erreichbar.",
        Key::ErrorPermissionDenied => "Keine Berechtigung für den Dienst.",
        Key::ErrorBusy => "Der Dienst ist gerade beschäftigt.",
        Key::ErrorTimeout => "Der Dienst hat nicht rechtzeitig geantwortet.",
        Key::ErrorConnectionLost => "Die Verbindung zum Dienst ist abgerissen.",
        Key::ErrorStatus => "Der Dienst meldet HTTP-Fehler {0}.",
        Key::ErrorInvalidResponse => "Die Antwort des Dienstes ist unlesbar.",
        Key::ErrorResponseTooLarge => "Die Antwort des Dienstes ist zu groß.",
        Key::ErrorService => "Der Dienst meldet: {0}",
        Key::ErrorInvalidHost => "Der Servername ist ungültig.",
        // --- Streaming (streaming.rs) ---
        Key::StreamingName => "Streaming {0}",
        Key::StreamingLabel => "{0} ({1})",
        // --- Field errors (settings.rs) ---
        Key::FieldNameError => "Nur Buchstaben, Ziffern, Punkt, Bindestrich, Unterstrich und * erlaubt.",
        Key::FieldNetworkError => "Netz in der Form 192.168.1.0/24 angeben.",
        Key::FieldDnsError => "IP-Adresse, optional mit Port, z. B. 9.9.9.9:53.",
        Key::FieldStampError => "Eintrag muss ein DNS-Stamp (sdns://…) sein.",
        Key::FieldBackupError => "Nur ein Dateiname; die Sicherung liegt immer in /run/hide.me.",
        Key::FieldInterfaceError => "1 bis 15 Zeichen aus Buchstaben, Ziffern, Punkt, Bindestrich und Unterstrich.",
        Key::FieldInterfaceTaken => "Diesen Namen hat schon eine andere Netzwerkschnittstelle.",
        Key::FieldTableReserved => "Tabellen 253 bis 255 gehören dem System.",
        Key::FieldRange => "Zahl zwischen {0} und {1}.",
        Key::FieldPgError => "Nur Aus, ab 12, ab 18 oder ab 21.",
        Key::FieldRiskUnknown => "Unbekannte Risikostufe.",
        Key::FieldIllegalUnknown => "Unbekannte Art illegaler Inhalte.",
        Key::FieldCategoryUnknown => "Unbekannte Kategorie.",
        Key::FieldNameInBothLists => "Ein Name darf nicht in beiden Listen stehen.",
        Key::FieldDnsRequired => "Mindestens ein DNS-Server ist nötig.",
        Key::FieldIpVersionUnknown => "Unbekannte IP-Version.",
        Key::FieldNotSelectable => "Dieser Eintrag ist nicht wählbar.",
        Key::FieldUnknownSetting => "Unbekannte Einstellung „{0}“.",

        // --- State and file of the settings (settings.rs) ---
        Key::SettingsVersionNotice => {
            "Diese Einstellungen sind nur mit hide.me {0} geprüft. Setze sie auf Standard oder aktualisiere \
             Celata."
        }
        Key::UntestedVersion => "{0} Der Dienst meldet Version {1}.",
        Key::SettingsAppliesNow => "Gilt für die laufende Verbindung.",
        Key::SettingsAppliesNext => "Gilt ab der nächsten Verbindung.",
        Key::SettingsFileBroken => {
            "Die gespeicherten Einstellungen ließen sich nicht lesen. Es gelten die Standardwerte; die Datei \
             bleibt bis zur nächsten Änderung unverändert."
        }
        Key::SettingsFileNewer => {
            "Die gespeicherten Einstellungen stammen von einer neueren Celata-Version. Es gelten die \
             Standardwerte; die Datei bleibt bis zur nächsten Änderung unverändert."
        }

        // --- Warnings before applying (settings.rs) ---
        Key::WarningRebuild => {
            "Beim Übernehmen ist das Internet kurz ungeschützt, auch mit Internet-Sperre. Scheitert die neue \
             Verbindung, geschieht das beim Zurückwechseln ein zweites Mal."
        }
        Key::WarningKillSwitch => "Ohne Internet-Sperre fließen deine Daten ungeschützt, wenn das VPN ausfällt.",
        Key::WarningPortForward => "Geräte aus dem Internet können dann Verbindungen zu diesem Rechner aufbauen.",
        Key::WarningSplit => "Verkehr in diese Netze läuft nicht durch das VPN.",
        Key::WarningWideSplit => "Damit läuft fast der ganze Verkehr am VPN vorbei.",
        Key::WarningResolver => "Diese Server sehen, mit welchem hide.me-Server du dich verbindest.",
        Key::WarningDohOff => {
            "Ohne DNS-over-HTTPS sehen Rückfall-DNS-Server und dein Netz, mit welchem hide.me-Server du dich \
             verbindest."
        }
        Key::WarningAdvanced => "Falsche Werte können Netzwerk und DNS dieses Rechners stören.",
        Key::WarningResolvBackup => {
            "Die Sicherung von resolv.conf kann frühere DNS-Einstellungen überschreiben, wenn sie nicht \
             zurückgespielt wird."
        }
        Key::SentencePair => "{0} {1}",
        Key::ParagraphPair => "{0}\n\n{1}",

        // --- Hints in the overview (settings.rs) ---
        Key::HintKillSwitchOff => "Internet-Sperre ist ausgeschaltet. Fällt das VPN aus, fließen Daten ungeschützt.",
        Key::HintSplitTunnel => "Split-Tunnel aktiv: {0}",
        Key::HintPortForward => "Port-Weiterleitung ist angefordert.",
        Key::HintDohOff => "DNS-over-HTTPS ist aus: Der VPN-Server wird über einfache DNS-Anfragen gesucht.",
        Key::HintPlainDns => "Eigene Rückfall-DNS-Server: {0}",
        Key::HintAdvanced => "Werte unter „Erweitert“ weichen vom Standard ab.",
        Key::HintDnsLeak => {
            "DNS-Leck möglich: Dieses System löst Namen über systemd-resolved auf, DNS-Anfragen können am VPN vorbeilaufen. Abhilfe: Abschnitt „DNS leaks“ in der Dokumentation von Celata."
        }
        // --- States (state.rs) ---
        Key::StateCheckingTitle => "Zustand wird geprüft …",
        Key::StateCheckingText => "Celata fragt den hide.me-Dienst ab.",
        Key::StateNotConnectedTitle => "Nicht verbunden",
        Key::StateNotConnectedText => {
            "Dein Internet läuft normal, aber ohne VPN. Wähle einen Server, um dich zu verbinden."
        }
        Key::StateConnectingTitle => "Wird verbunden …",
        Key::StateConnectingText => "Die Verbindung wird aufgebaut. Bitte einen Moment warten.",
        Key::StateConnectingBlockedTitle => "Wird verbunden, Internet gesperrt",
        Key::StateConnectingBlockedText => {
            "Der Dienst stellt die Verbindung wieder her. Bis dahin sperrt der Kill-Switch dein Internet zum \
             Schutz. Warte kurz oder hebe die Sperre auf. Zum Aufheben klickst du auf das rote Feld rechts im \
             Knopf unten."
        }
        Key::StateConnectingToTitle => "Verbinde mit {0} …",
        Key::StateConnectingBlockedToTitle => "Verbinde mit {0}, Internet gesperrt",
        Key::StateConnectedTitle => "Verbunden",
        Key::StateConnectedText => "Dein Datenverkehr läuft geschützt über den VPN-Server.",
        Key::StateDisconnectingTitle => "Wird getrennt …",
        Key::StateDisconnectingText => "Die Verbindung wird beendet. Bitte einen Moment warten.",
        Key::StateSwitchingTitle => "Wechsel zu {0} …",
        Key::StateSwitchingText => {
            "Celata verbindet dich mit dem neuen Server. Bis dahin ist dein Internet zum Schutz gesperrt."
        }
        Key::StateReturningTitle => "Zurück zu {0} …",
        Key::StateReturningText => {
            "Der gewählte Server war nicht erreichbar. Celata verbindet dich wieder mit dem vorherigen."
        }
        Key::StateReapplyingTitle => "Einstellungen werden übernommen …",
        Key::StateReapplyingText => {
            "Celata verbindet neu, damit die geänderten Einstellungen gelten. Bitte einen Moment warten."
        }
        Key::StateReapplyingBackTitle => "Vorherige Einstellungen werden wiederhergestellt …",
        Key::StateReapplyingBackText => {
            "Mit den neuen Einstellungen kam keine Verbindung zustande. Celata verbindet mit den vorherigen."
        }
        Key::StateBlockedTitle => "Internet gesperrt",
        Key::StateBlockedText => {
            "Es besteht keine VPN-Verbindung, und der Kill-Switch sperrt dein Internet zum Schutz. Verbinde dich \
             neu oder hebe die Sperre auf. Zum Aufheben klickst du auf das rote Feld rechts im Knopf unten."
        }
        Key::StateServiceUnreachableTitle => "Dienst nicht erreichbar",
        Key::StateServiceUnreachableText => {
            "Der hide.me-Dienst läuft nicht. Starte ihn in der Konsole mit diesem Befehl. Celata erkennt den \
             Start von selbst:"
        }
        Key::StateServiceUnreachableBlockedTitle => "Dienst nicht erreichbar, Internet gesperrt",
        Key::StateServiceUnreachableBlockedText => {
            "Der Dienst antwortet nicht, aber die Sperre ist noch aktiv. Beende den Dienst in der Konsole mit \
             diesem Befehl, dann ist dein Internet wieder frei:"
        }
        Key::StateNoPermissionTitle => "Keine Berechtigung",
        Key::StateNoPermissionText => {
            "Dein Benutzer darf den Dienst nicht steuern: Er gehört nicht zur Gruppe „celata“, oder du hast \
             dich seit der Aufnahme noch nicht neu angemeldet. Nimm ihn mit diesem Befehl auf und melde dich \
             danach einmal ab und wieder an:"
        }
        Key::StateServiceFaultyTitle => "Dienst antwortet nicht richtig",
        Key::StateServiceFaultyText => {
            "Der hide.me-Dienst läuft, antwortet aber nicht wie erwartet. Celata versucht es jede Sekunde neu. \
             Bleibt die Meldung, starte den Dienst in der Konsole neu:"
        }
        Key::DetailReason => "Grund: {0}",
        Key::VersionWarning => {
            "Der Dienst meldet Version {0}, geprüft ist Celata mit {1}. Einzelne Anzeigen können abweichen."
        }

        // --- Question on closing (state.rs) ---
        Key::CloseConnectedText => "Das VPN ist verbunden. Soll die Verbindung beim Schließen getrennt werden?",
        Key::CloseBusyText => {
            "Die Verbindung wird gerade auf- oder abgebaut. Scheitert der Aufbau nach dem Schließen, kann das \
             Internet gesperrt bleiben; Celata zeigt das beim nächsten Öffnen an."
        }
        Key::CloseBlockedText => {
            "Dein Internet ist gesperrt. Wenn du Celata jetzt schließt, ohne zu trennen, bleibt die Sperre \
             bestehen. „Trennen und schließen“ hebt sie auf; danach fließen deine Daten ohne VPN."
        }
        Key::CloseOfflineBlockedText => {
            "Dein Internet ist gesperrt, und der Dienst antwortet nicht. Nach dem Schließen hebst du die Sperre \
             in der Konsole mit diesem Befehl auf:"
        }
        Key::CloseKeepConnected => "Verbunden lassen und schließen",
        Key::CloseKeepBlocked => "Gesperrt lassen und schließen",
        Key::CloseAnyway => "Trotzdem schließen",

        // --- Notices after actions (state.rs) ---
        Key::NoticeConnectNotStarted => "Die Verbindung wurde nicht gestartet. Versuche es gleich noch einmal.",
        Key::NoticeConnectRejected => {
            "Der Dienst hat die Einstellungen abgelehnt, deshalb wurde nicht verbunden. Prüfe die Einstellungen. \
             Grund: {0}"
        }
        Key::NoticeConnectFailedReleased => {
            "Die Verbindung konnte nicht aufgebaut werden. Dein Internet ist wieder frei, es ist aber kein VPN \
             aktiv. Wähle einen Server und versuche es erneut."
        }
        Key::NoticeConnectFailedBlocked => {
            "Die Verbindung konnte nicht aufgebaut werden. Die Sperre bleibt zu deinem Schutz bestehen. Versuche \
             es mit einem anderen Server oder hebe die Sperre auf."
        }
        Key::NoticeConnectReleaseFailed => {
            "Die Verbindung konnte nicht aufgebaut werden, und das Internet ließ sich nicht wieder freigeben. \
             Beende den Dienst in der Konsole mit diesem Befehl:"
        }
        Key::NoticeRouteRejected => {
            "Das Internet wurde nicht gesperrt: Der Dienst hat die Einstellungen abgelehnt ({0})."
        }
        Key::NoticeRouteFailed => "Das Internet ließ sich nicht sperren.",
        Key::RouteHint => "Dafür muss die Internet-Sperre in den Einstellungen eingeschaltet sein.",
        Key::NoticeUnblocked => "Die Sperre ist aufgehoben. Dein Internet läuft jetzt ohne VPN.",
        Key::NoticeUnblockFailed => {
            "Die Sperre ließ sich nicht aufheben. Beende den Dienst in der Konsole mit diesem Befehl, das hebt \
             die Sperre sicher auf:"
        }
        Key::NoticeCloseDisconnectFailed => {
            "„Trennen und schließen“ ist gescheitert, deshalb bleibt Celata offen. Beende den Dienst in der \
             Konsole mit diesem Befehl, dann ist die Verbindung getrennt und das Internet frei:"
        }
        Key::NoticeDisconnectFailed => {
            "Die Verbindung ließ sich nicht trennen. Beende den Dienst in der Konsole mit diesem Befehl, dann ist \
             die Verbindung getrennt und das Internet frei:"
        }
        Key::UnblockWarning => {
            "Wenn du die Sperre aufhebst, ist kein VPN aktiv: Deine Daten fließen ungeschützt über deine normale \
             Internetverbindung, bis du dich wieder verbindest. Sperre trotzdem aufheben?"
        }

        // --- Reconnecting after a change of settings (state.rs) ---
        Key::NoticeReapplied => "Verbindung mit den neuen Einstellungen neu hergestellt.",
        Key::NoticeReapplyLater => "Sie gelten ab der nächsten Verbindung.",
        Key::NoticeReapplyReturned => {
            "Neue Einstellungen nicht übernommen, Verbindung mit den vorherigen Einstellungen wiederhergestellt: \
             {0}"
        }
        Key::NoticeReapplyRejected => {
            "Neue Einstellungen nicht übernommen: Der Dienst hat sie abgelehnt ({0}). Die Verbindung besteht \
             unverändert. Prüfe die Einstellungen."
        }
        Key::NoticeReapplyRefused => {
            "Neue Einstellungen nicht übernommen: Die Internet-Sperre ist nicht bereit oder nicht prüfbar, ohne \
             sie wäre dein Internet beim Neu verbinden kurz ohne VPN. {0}"
        }
        Key::NoticeReapplyUnreachable => {
            "Neue Einstellungen nicht übernommen: Der Server ist gerade nicht erreichbar. {0}"
        }
        Key::NoticeReapplyNotStarted => "Neue Einstellungen nicht übernommen. {0}",
        Key::NoticeReapplyStranded => {
            "Neu verbinden ist gescheitert, auch mit den vorherigen Einstellungen. Wähle einen Server oder hebe \
             die Sperre auf."
        }
        Key::NoticeReapplyUnclear => "Neu verbinden ließ sich nicht starten.",
        // --- Switching servers (switch.rs) ---
        Key::NoticeSwitchReturned => {
            "„{0}“ war nicht erreichbar. Du bist wieder mit „{1}“ verbunden. Wähle einen anderen Server, wenn du \
             wechseln möchtest."
        }
        Key::NoticeSwitchStranded => {
            "Der Wechsel ist gescheitert, auch „{0}“ war nicht erreichbar. Wähle einen anderen Server oder hebe \
             die Sperre auf."
        }
        Key::NoticeSwitchRefused => {
            "Wechsel nicht möglich: Die Internet-Sperre des Dienstes ist nicht aktiv oder nicht prüfbar. Ohne sie \
             wäre dein Internet während des Wechsels kurz ohne VPN. Trenne zuerst, wenn du das in Kauf nimmst."
        }
        Key::NoticeSwitchUnreachable => {
            "„{0}“ ist gerade nicht erreichbar. Du bleibst mit „{1}“ verbunden. Versuche es später oder wähle \
             einen anderen Server."
        }
        Key::NoticeSwitchUnclear => "Der Wechsel ließ sich nicht starten.",
        Key::NoticeSwitchRejected => "Der Dienst hat die Einstellungen abgelehnt: {0}",
        // --- Server list and locations (servers.rs, look.rs) ---
        Key::ServerAutomatic => "Automatisch",
        Key::ServerFree => "Kostenloser Server",
        Key::ServerPlain => "Server",
        Key::AutomaticPlace => "Automatisch, hide.me wählt den Ort",
        Key::StreamingPlace => "Streaming",
        Key::LocationChoice => "{0}, {1}",
        Key::ListHeadingFavorites => "Favoriten",
        Key::ListHeadingAll => "Alle Standorte",
        Key::TargetLabel => "{0} ({1})",
        // --- Best location (best.rs, look.rs) ---
        Key::BestName => "Bester Standort",
        Key::BestHint => "Automatisch, Celata wählt den Standort",
        Key::BestChosen => "Gewählt: {0}",
        Key::NoticeBestAlready => "Bereits mit dem besten Standort verbunden.",
        Key::NoticeBestUnknown => "Bester Standort lässt sich nicht ermitteln, bitte einen Standort wählen.",
        Key::NoticeBestLoading => "Die Standorte werden noch geladen. Bitte gleich noch einmal versuchen.",
        Key::NoticeBestNoList => {
            "Es sind keine Standorte geladen. Bitte die Liste neu laden oder später einen Standort wählen."
        }

        // --- Main button (look.rs) ---
        Key::ButtonConnect => "Verbinde VPN",
        Key::ButtonDisconnect => "Trennen",
        Key::ButtonReconnecting => "Wird neu verbunden …",
        Key::ButtonSwitching => "Wird gewechselt …",
        Key::ButtonConnecting => "Wird verbunden …",
        Key::ButtonDisconnecting => "Wird getrennt …",
        Key::ButtonBlocking => "Internet wird gesperrt …",
        Key::ButtonBlock => "Internet sperren",
        Key::ButtonUnblock => "Sperre aufheben",

        // --- Public address (look.rs) ---
        Key::ExternalIpAsking => "wird ermittelt …",
        Key::ExternalIpFailed => "nicht ermittelbar",
        Key::TrayTooltip => "Celata – {0}",
        // --- Tray menu (look.rs, feature 014) ---
        Key::TrayShow => "Celata anzeigen",
        Key::TrayBest => "Mit bestem Standort verbinden",
        Key::TrayFavorites => "Favoriten",
        Key::TrayNoFavorites => "Keine Favoriten",
        Key::TrayFavoriteAdd => "Aktuellen Standort zu Favoriten hinzufügen",
        Key::TrayFavoriteRemove => "Aktuellen Standort aus Favoriten entfernen",
        Key::TrayFavoriteUnavailable => "Standort nicht verfügbar",
        Key::TrayFavoriteLoading => "Wird geladen …",
        Key::TrayLocations => "Standorte",
        Key::ContinentEurope => "Europa",
        Key::ContinentAsia => "Asien",
        Key::ContinentNorthAmerica => "Nordamerika",
        Key::ContinentSouthAmerica => "Südamerika",
        Key::ContinentAfrica => "Afrika",
        Key::ContinentOceania => "Ozeanien",
        Key::TrayLocationsLoading => "Standorte werden geladen …",
        Key::TrayLocationsMissing => "Standorte nicht verfügbar",
        Key::TrayStreaming => "Streaming",
        Key::TrayStreamingPlace => "{0} (Streaming)",
        Key::TrayAutomatic => "Automatisch",
        Key::TrayFreePlace => "{0} (kostenlos)",
        Key::TrayAbout => "Über Celata",
        Key::TrayQuit => "Beenden",
        Key::UpdateAvailable => "Celata {0} ist verfügbar",
        Key::UpdateBlockedNotice => {
            "Das Internet ist gesperrt. Die Seite lädt, sobald das VPN verbunden oder die Sperre aufgehoben ist."
        }
        Key::AboutVersion => "Version {0}",

        // --- Notifications (notify.rs, feature 014) ---
        Key::NotifyConnected => "Verbunden: {0}",
        Key::NotifyDisconnected => "Getrennt",
        Key::NotifyLost => "Verbindung abgebrochen",
        Key::NotifyLostBlocked => "Verbindung abgebrochen, Internet gesperrt",
        Key::NotifyFailed => "Verbinden mit {0} gescheitert",
        Key::NotifySwitched => "Gewechselt zu {0}",
        Key::NotifySwitchedBack => "Wechsel gescheitert, zurück zu {0}",
        Key::NotifyBlocked => "Internet gesperrt",
        Key::NotifyUnblocked => "Sperre aufgehoben",
        Key::NotifyActionFailed => "Aktion gescheitert. Einzelheiten im Fenster von Celata.",

        // --- Settings of Celata, section "Application" (main.rs, feature 014) ---
        Key::SettingsAutostartForeign => "Ein anderer Eintrag startet Celata bei der Anmeldung ({0}).",
        Key::SettingsAutostartFailed => "Der Eintrag für den Start bei der Anmeldung ließ sich nicht ändern.",

        // --- Command line (cli.rs, unblock.rs, feature 014) ---
        Key::CliHelp => {
            "Aufruf: celata [OPTION]\n\nOhne Option öffnet Celata sein Fenster.\n\n  --version      Gibt die Version aus.\n  --unblock      Hebt die Internet-Sperre auf, ohne das Fenster zu öffnen.\n  --polkit-rule  Gibt die Regel aus, mit der Mitglieder der Gruppe celata den Dienst starten dürfen.\n  --help         Zeigt diese Hilfe."
        }
        Key::CliUnknown => "Unbekannte Option: {0}. Siehe celata --help.",
        Key::CliFreed => "Internet freigegeben.",
        Key::CliNotBlocked => "Das Internet ist nicht gesperrt.",
        Key::CliUnreadable => "Die Routing-Tabelle ließ sich nicht lesen, deshalb hat Celata nichts geändert.",
        Key::CliFailed => "Die Sperre ließ sich nicht aufheben. {0}",
        Key::CliEmergency => "Notausgang: sudo systemctl stop celata-hide.me.service",
        Key::StateServiceStartingTitle => "Dienst wird gestartet …",
        Key::StateServiceStartingText => {
            "Celata startet den hide.me-Dienst. Nach dem Anmelden kann das einige Sekunden dauern, bis das Netzwerk \
             bereit ist."
        }
        Key::StateServiceStartingBlockedTitle => "Dienst wird gestartet, Internet gesperrt",
        Key::StateServiceStartingBlockedText => {
            "Celata startet den hide.me-Dienst. Der Kill-Switch sperrt dein Internet weiter; Celata hebt die Sperre \
             nicht von selbst auf."
        }
        Key::StateServiceUnreachableAutoText => {
            "Der hide.me-Dienst läuft nicht. Celata startet ihn, sobald du verbindest."
        }
        Key::StartNeedsPasswordTitle => "Dienst konnte nicht gestartet werden",
        Key::StartNeedsPasswordText => {
            "Celata darf den hide.me-Dienst auf diesem Rechner nicht ohne Passwort starten. Klicke auf „Dienst \
             starten“, dann fragt das System nach dem Administrator-Passwort."
        }
        Key::StartNotInstalledTitle => "Dienst nicht eingerichtet",
        Key::StartNotInstalledText => {
            "Der hide.me-Dienst für Celata ist auf diesem Rechner nicht eingerichtet. Installiere das Celata-Paket oder \
             folge der Anleitung zur Einrichtung."
        }
        Key::StartFailedText => {
            "Der hide.me-Dienst wurde gestartet, hat aber gleich wieder aufgehört. Häufige Gründe: Der hide.me-Client \
             fehlt oder hat noch keinen Access-Token. Die letzten Meldungen des Dienstes zeigt dieser Befehl:"
        }
        Key::StartNoPromptNotice => {
            "Das System konnte nicht nach dem Passwort fragen. Starte den Dienst in der Konsole:"
        }
        Key::SetupGuideBlockedNotice => {
            "Das Internet ist gesperrt. Die Anleitung öffnet sich, sobald die Sperre aufgehoben ist oder eine \
             VPN-Verbindung besteht."
        }
        Key::StartFailedTitle => "Dienst startet nicht",
        // END OF KEYS (marker: add new texts before this line)
    }
}

/// German country names (ISO 3166-1, upper case).
pub(super) fn country(code: &str) -> Option<&'static str> {
    Some(match code {
        "AE" => "Vereinigte Arabische Emirate",
        "AL" => "Albanien",
        "AM" => "Armenien",
        "AR" => "Argentinien",
        "AT" => "Österreich",
        "AU" => "Australien",
        "AZ" => "Aserbaidschan",
        "BA" => "Bosnien und Herzegowina",
        "BD" => "Bangladesch",
        "BE" => "Belgien",
        "BG" => "Bulgarien",
        "BO" => "Bolivien",
        "BR" => "Brasilien",
        "BY" => "Belarus",
        "CA" => "Kanada",
        "CH" => "Schweiz",
        "CL" => "Chile",
        "CN" => "China",
        "CO" => "Kolumbien",
        "CR" => "Costa Rica",
        "CY" => "Zypern",
        "CZ" => "Tschechien",
        "DE" => "Deutschland",
        "DK" => "Dänemark",
        "EC" => "Ecuador",
        "EE" => "Estland",
        "EG" => "Ägypten",
        "ES" => "Spanien",
        "FI" => "Finnland",
        "FR" => "Frankreich",
        "GB" => "Vereinigtes Königreich",
        "GE" => "Georgien",
        "GR" => "Griechenland",
        "HK" => "Hongkong",
        "HR" => "Kroatien",
        "HU" => "Ungarn",
        "ID" => "Indonesien",
        "IE" => "Irland",
        "IL" => "Israel",
        "IN" => "Indien",
        "IS" => "Island",
        "IT" => "Italien",
        "JP" => "Japan",
        "KE" => "Kenia",
        "KH" => "Kambodscha",
        "KR" => "Südkorea",
        "KZ" => "Kasachstan",
        "LK" => "Sri Lanka",
        "LT" => "Litauen",
        "LU" => "Luxemburg",
        "LV" => "Lettland",
        "MA" => "Marokko",
        "MD" => "Moldau",
        "ME" => "Montenegro",
        "MK" => "Nordmazedonien",
        "MT" => "Malta",
        "MX" => "Mexiko",
        "MY" => "Malaysia",
        "NG" => "Nigeria",
        "NL" => "Niederlande",
        "NO" => "Norwegen",
        "NP" => "Nepal",
        "NZ" => "Neuseeland",
        "PA" => "Panama",
        "PE" => "Peru",
        "PH" => "Philippinen",
        "PK" => "Pakistan",
        "PL" => "Polen",
        "PT" => "Portugal",
        "QA" => "Katar",
        "RO" => "Rumänien",
        "RS" => "Serbien",
        "SA" => "Saudi-Arabien",
        "SE" => "Schweden",
        "SG" => "Singapur",
        "SI" => "Slowenien",
        "SK" => "Slowakei",
        "TH" => "Thailand",
        "TR" => "Türkei",
        "TW" => "Taiwan",
        "UA" => "Ukraine",
        "US" => "USA",
        "UY" => "Uruguay",
        "VN" => "Vietnam",
        "ZA" => "Südafrika",
        _ => return None,
    })
}

/// German names of places whose name is different in English.
pub(super) fn place(original: &str) -> Option<&'static str> {
    Some(match original {
        "Athens" => "Athen",
        "Belgrade" => "Belgrad",
        "Bogota" => "Bogotá",
        "Brussels" => "Brüssel",
        "Bucharest" => "Bukarest",
        "California" => "Kalifornien",
        "Cairo" => "Kairo",
        "Chisinau" => "Chișinău",
        "Cologne" => "Köln",
        "Copenhagen" => "Kopenhagen",
        "Florence" => "Florenz",
        "Geneva" => "Genf",
        "Ho Chi Minh City" => "Ho-Chi-Minh-Stadt",
        "Hong Kong" => "Hongkong",
        "Kyiv" => "Kiew",
        "Lisbon" => "Lissabon",
        "Milan" => "Mailand",
        "Moscow" => "Moskau",
        "Munich" => "München",
        "Naples" => "Neapel",
        "Prague" => "Prag",
        "Queretaro" => "Querétaro",
        "Rome" => "Rom",
        "Siauliai" => "Šiauliai",
        "Singapore" => "Singapur",
        "Taipei" => "Taipeh",
        "Tokyo" => "Tokio",
        "Venice" => "Venedig",
        "Vienna" => "Wien",
        "Warsaw" => "Warschau",
        "Zurich" => "Zürich",
        _ => return None,
    })
}
