#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-only
# Copyright (C) 2026 derHouwer
#
# celata-setup.sh - sets up Celata on a supported Linux system.
#
# What it does: finds the hide.me client, installs the Celata package of the same version with the package
# manager of the system (after checking it against SHA256SUMS), adds a drop-in for the service if the client
# lies somewhere else than the package expects, and adds you to the group "celata".
# What it never does: install or change the hide.me client, touch the DNS settings, start or restart the
# service, connect, or block the internet.
#
# Usage:
#   bash celata-setup.sh [--yes] [--user NAME] [--client PATH]    set up or repair, with questions
#   bash celata-setup.sh --check [--user NAME] [--client PATH]    show what is in place, change nothing
#   bash celata-setup.sh --dry-run [--user NAME] [--client PATH]  show what would be done, change nothing
#   bash celata-setup.sh --uninstall [--yes] [--dry-run]          remove what this script set up
#   bash celata-setup.sh --help | --version
#
# When this file is loaded with "source", it only defines functions and tables and runs nothing.

set -Eeuo pipefail
IFS=$'\n\t'

# --- Constants ---------------------------------------------------------------------------------------------

CELATA_VERSION=0.9.0
CELATA_PKGREL=1
# SHA-256 of packaging/polkit/49-celata.rules; a rule installed by hand is compared with it.
CELATA_RULE_SHA256=00986ebe87c4fb5466c48bf023abcbbc826161a73c65d26794c093496539d60f

RELEASE_URL_BASE=https://github.com/derHouwer/celata/releases/download/v
EXPECTED_CLIENT_VERSION=0.9.14

UNIT=celata-hide.me.service
GROUP=celata
CONTROL_SOCKET=/run/hide.me/control.sock
BLOCK_TABLE=55555
STATE_DIR=/var/lib/celata-setup
STATE_FILE=$STATE_DIR/state
DROPIN_DIR=/etc/systemd/system/$UNIT.d
DROPIN_FILE=$DROPIN_DIR/50-celata-setup.conf
DROPIN_MARK='# X-Celata-Setup=1'
MANUAL_UNIT=/etc/systemd/system/$UNIT
PACKAGE_UNIT=/usr/lib/systemd/system/$UNIT
PACKAGE_SYSUSERS=/usr/lib/sysusers.d/celata.conf
PACKAGE_RULE=/usr/share/polkit-1/rules.d/49-celata.rules
ETC_RULES_DIR=/etc/polkit-1/rules.d
ETC_RULE=$ETC_RULES_DIR/49-celata.rules
MANUAL_PROGRAM=/usr/local/bin/celata
CLIENT_PLACES=(/opt/hide.me/hide.me /usr/bin/hide.me /usr/local/bin/hide.me)
CLIENT_DIR_PLACES=(/etc/hide.me /opt/hide.me)
MIN_POLKIT=0.106
MIN_UID=1000
MAX_UID=60000

# Return values
RC_OK=0
RC_OPEN=1
RC_USAGE=2
RC_ABORT_CLEAN=3
RC_USER_ABORT=4

# --- Run state (set while the script runs; the tests set them directly) -----------------------------------

SCRIPT_PATH=${BASH_SOURCE[0]}
SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)

OPT_YES=0
OPT_CHECK=0
OPT_DRY=0
OPT_UNINSTALL=0
OPT_HELP=0
OPT_VERSION=0
OPT_USER=''
OPT_CLIENT=''

LANG_CODE=en
UTF8=0
USER_NAME=''
WORKDIR=''
PKGDIR=''
USE_ROOT_FOR_RULE=0

# System description and classification (read_os_release, classify)
OS_ID='' OS_LIKE='' OS_VERSION='' OS_CODENAME='' OS_UBUNTU_CODENAME='' OS_DEBIAN_CODENAME=''
OS_PLATFORM='' OS_VARIANT='' OS_PRETTY=''
HAS_SYSTEMD=1
IS_OSTREE=0
MACHINE=x86_64
CL_BASE='' CL_BASE_VERSION='' CL_FAMILY='' CL_KIND=unsupported CL_REASON=''

# Client
CLIENT_PATH='' CLIENT_DIR='' CLIENT_STATE=missing CLIENT_BAD_PATH=''
CLIENT_LIST=()
GROUP_LINE=''
PM_ARGV=()

# Facts the plan is built from
PKG_STATE=none            # none | old | current | newer
PKG_VERSION=''
PKG_FILES_MISSING=0
MANUAL_UNIT_PRESENT=0
MANUAL_RULE_PRESENT=0     # 0 | 1 | unknown
MANUAL_PROGRAM_PRESENT=0
DROPIN_STATE=absent       # absent | current | stale | foreign
GROUP_PRESENT=0
MEMBER_PRESENT=0
UNIT_ACTIVE=0
PKG_PATH='' PKG_FROM='' EXPECTED_SUM=''

# Plan and results of a run
PLAN_ID=() PLAN_ROOT=() PLAN_CONSENT=() PLAN_CMD=()
DONE_STEPS=() SKIPPED_STEPS=()
# Steps with a question of their own that were answered already; the plan may be built again after that.
ASKED_YES=() ASKED_NO=()
NEED_RELOGIN=0
NEED_RESTART=0
CHANGES_MADE=0

declare -A CHECK_STATUS=() CHECK_DETAIL=() CHECK_NEXT=()
declare -A STATE=()
declare -A UNIT_INFO=()
STATE_EXTRA=()
CHECK_ORDER=(system blocked client client_version token package unit group member session polkit dns tray other_service)

# BEGIN TEXTS
declare -A TEXT_EN=(
    [usage_short]="Usage: bash celata-setup.sh [--yes] [--user NAME] [--client PATH] | --check | --dry-run | --uninstall | --help | --version"
    [help_text]="Sets up Celata: finds the hide.me client, installs the Celata package of this version and adds you to the group celata.\nIt never installs the client, never touches DNS, never starts the service and never connects or blocks the internet.\n\n  bash celata-setup.sh [--yes] [--user NAME] [--client PATH]    set up or repair, with questions\n  bash celata-setup.sh --check [--user NAME] [--client PATH]    show what is in place, change nothing\n  bash celata-setup.sh --dry-run [--user NAME] [--client PATH]  show what would be done, change nothing\n  bash celata-setup.sh --uninstall [--yes] [--dry-run]          remove what this script set up\n  bash celata-setup.sh --help | --version\n\n  --yes      agree to the plan and to the group without asking (never to replacing a file or a separate package)\n  --user     the account to set up (needed when you run the script as root)\n  --client   the place of the hide.me client, if it is not at a known place or if there are several"
    [version_line]="celata-setup %s-%s"
    [err_duplicate]="%s was given twice"
    [err_unknown_option]="Unknown option or argument: %s"
    [err_user_value]="--user needs an account name, for example --user anna"
    [err_client_value]="--client needs the full path of the client, for example --client /usr/local/bin/hide.me"
    [err_combination_check]="--check cannot be combined with --dry-run, --uninstall or --yes"
    [err_combination_uninstall]="--uninstall cannot be combined with --client"
    [err_bash_old]="This script needs bash 5.0 or newer (found %s)."
    [err_internal]="Internal error in line %s. Nothing more was done. A new run starts from the current state."
    [aborted_by_user]="Aborted. A new run completes the setup."
    [choice_yes]="[Y/n]"
    [choice_no]="[y/N]"
    [answer_auto_yes]="yes (--yes)"
    [err_no_terminal]="There is no terminal to ask. Run the script in a terminal, or add --yes."
    [ask_user]="You are running this as root. For which account should Celata be set up?"
    [err_user_needed]="Tell the script the account with --user NAME. Nothing was changed."
    [err_user_invalid]="%s cannot be used: it must be an existing account that is not root and has a normal user number."
    [ask_derivative]="Continue?"
    [derivative_note]="%s is set up as %s. This system is not tested."
    [dry_derivative_note]="%s would be set up as %s. This system is not tested. A real run asks first."
    [ask_execute]="Run this now?"
    [client_choose_intro]="Several hide.me clients were found:"
    [ask_client_number]="Which one should Celata use? Enter a number from 1 to %s:"
    [err_client_choice]="That is not a valid choice. Nothing was changed."
    [ask_install_beside_manual]="%s: Celata is set up by hand on this system, and the package brings its own service file and rule. Do it anyway?"
    [ask_update_preexisting]="%s: the package was installed before this script. Update it?"
    [ask_reinstall_preexisting]="%s: the package was installed before this script. Do it?"
    [ask_remove_preexisting]="%s: the package was installed before this script. Remove it?"
    [ask_dropin_replace]="%s: a file of that name exists already. Replace it?"
    [ask_dropin_remove]="%s: the file is not needed any more. Remove it?"
    [ask_step_generic]="%s: do it?"
    [plan_asks_separately]="(asks on its own)"
    [header]="Celata %s setup"
    [header_system]="System: %s"
    [all_set]="Everything is set up. There is nothing to do."
    [plan_intro]="This is what will be done:"
    [plan_sudo_note]="Steps with administrator rights use sudo, one command at a time. It may ask for your password."
    [preflight_announce]="I am checking now whether everything is ready: %s. Nothing is installed yet."
    [preflight_sudo_note]="sudo may ask for your password now."
    [preflight_downloading]="Downloading %s ..."
    [part_lists]="renew the package lists (needs sudo)"
    [part_download]="download the Celata package"
    [part_probe]="try the package without installing it"
    [part_rule]="read %s (needs sudo)"
    [dry_would_download]="would download: %s"
    [dry_would_run]="would run: %s"
    [dry_package_checked]="package file %s is next to the script and matches SHA256SUMS"
    [summary_done]="Done:"
    [summary_open]="Open:"
    [summary_notes]="Notes:"
    [summary_nothing_missing]="Nothing is missing."
    [summary_relogin]="Log out and log in again once, so that the group takes effect."
    [summary_restart]="The changed service file applies after a restart of the service. If you are not connected, run: sudo systemctl restart %s"
    [chk_name_system]="System"
    [chk_name_blocked]="Internet"
    [chk_name_client]="hide.me client"
    [chk_name_client_version]="Client version"
    [chk_name_token]="Access-Token"
    [chk_name_package]="Celata package"
    [chk_name_unit]="Service file"
    [chk_name_group]="Group"
    [chk_name_member]="Membership"
    [chk_name_session]="Session"
    [chk_name_polkit]="Rule for polkit"
    [chk_name_dns]="DNS"
    [chk_name_tray]="Tray icon area"
    [chk_name_other_service]="Other hide.me service"
    [chk_system_tested]="%s (tested)"
    [chk_system_derivative]="%s as %s, not tested"
    [label_as]="%s as %s"
    [reason_architecture]="%s is not an x86_64 system"
    [reason_no-systemd]="%s does not run systemd"
    [reason_atomic]="%s is an image-based (atomic) system, the package cannot be installed there"
    [reason_opensuse]="%s (openSUSE) is not supported yet"
    [reason_old-release]="this release of %s is not supported"
    [reason_unknown]="%s is not a system this script knows"
    [next_unsupported]="Supported: Fedora 44 and 45, Debian 13, Ubuntu 24.04, 26.04 and 26.10, Linux Mint 22 and Arch Linux. Derivatives of these with a clear base can be set up, but are not tested. To set up by hand: https://github.com/derHouwer/celata/blob/main/docs/manual-setup.md"
    [chk_blocked_no]="not blocked"
    [chk_blocked_yes]="Internet blocked (the kill switch of the service is on)"
    [block_exit]="To unblock: click \"Unblock internet\" in Celata, run \"celata --unblock\", or run \"sudo systemctl stop %s\"."
    [err_blocked]="The internet is blocked, so nothing is downloaded or installed. Nothing was changed.\n%s"
    [chk_client_ok]="%s (working folder %s)"
    [chk_client_missing]="not found (looked at /opt/hide.me, /usr/bin and /usr/local/bin)"
    [chk_client_incomplete]="%s has no CA.pem beside it, so the client looks incomplete"
    [chk_client_ambiguous]="several found: %s"
    [chk_client_unsafe_path]="%s contains characters the script does not handle (only letters, digits and . _ + - / are allowed)"
    [chk_client_untrusted]="unsafe place: %s does not belong to root or others can change it"
    [next_client_missing]="Install the hide.me client first, as described at https://github.com/eventure/hide.client.linux. The script does not install it."
    [next_client_ambiguous]="Run the script again with --client PATH."
    [next_client_untrusted]="Check who changed it. If it is yours: sudo chown -R root:root %s and sudo chmod -R go-w %s"
    [err_client_ambiguous]="Several hide.me clients were found (%s). Choose one with --client PATH. Nothing was changed."
    [chk_version_ok]="%s"
    [chk_version_other]="%s, but Celata is tested with %s"
    [chk_version_unknown]="shown only while the service is running"
    [chk_token_ok]="present"
    [chk_token_missing]="no Access-Token found"
    [chk_token_unknown]="not checked (no client)"
    [next_token]="Create one as described in the guide of hide.me (https://github.com/eventure/hide.client.linux), command \"hide.me token\"."
    [chk_package_none]="not installed"
    [chk_package_nosystem]="not checked (system not supported)"
    [chk_package_manual]="not installed as a package, Celata is set up by hand"
    [chk_package_old]="version %s is older than this script (%s)"
    [chk_package_ok_setup]="version %s, installed by this script"
    [chk_package_ok_preexisting]="version %s, installed before this script"
    [chk_unit_ok]="fits the client"
    [chk_unit_absent]="no service file yet (it comes with the package)"
    [chk_unit_noclient]="not checked (no client)"
    [chk_unit_needs_dropin]="the client is not where the package expects it (%s), a drop-in fixes that"
    [chk_unit_differs]="the service file does not fit the client (%s)"
    [next_unit_differs]="The script does not change it. Drop-ins in use: %s"
    [chk_group_ok]="group %s exists"
    [chk_group_missing]="group %s does not exist yet"
    [chk_member_ok]="%s is a member"
    [chk_member_missing]="%s is not a member of the group %s"
    [chk_session_ok]="the group is active in this session"
    [chk_session_relogin]="log out and in again to activate the group"
    [chk_session_nomember]="not checked, you are not a member yet"
    [chk_session_root]="cannot be checked as root"
    [chk_polkit_package]="rule of the package in place"
    [chk_polkit_manual]="rule under /etc in place (made by hand)"
    [chk_polkit_absent]="no rule found (it comes with the package)"
    [chk_polkit_lost]="the rule of the package is missing"
    [chk_polkit_differs]="the rule under /etc differs from the one of Celata"
    [chk_polkit_unknown]="cannot be checked without administrator rights"
    [chk_polkit_old]="polkit %s is too old (%s or newer is needed)"
    [next_polkit_differs]="The script does not change a setup made by hand."
    [next_polkit_unknown]="Run: sudo bash %s --check"
    [err_polkit_old]="polkit %s is too old for the rule of Celata (%s or newer is needed). Nothing was changed."
    [chk_dns_none]="no systemd-resolved running"
    [chk_dns_leak]="systemd-resolved is running, DNS requests can bypass the VPN"
    [next_dns]="Celata shows a hint too, see https://github.com/derHouwer/celata/blob/main/docs/dns-leak.md. The script changes nothing here."
    [chk_tray_ok]="found"
    [chk_tray_missing]="none found, the icon will be missing, Celata runs anyway (GNOME needs an extension)"
    [chk_tray_unknown]="cannot be told (no session bus)"
    [chk_other_none]="no other hide.me service running"
    [chk_other_service]="%s is running"
    [next_other_service]="Both use the same interface. To stop it: sudo systemctl stop %s"
    [err_sums_missing]="SHA256SUMS is not next to this script, so %s cannot be checked. Download SHA256SUMS of the same release into this folder. Nothing was changed."
    [err_sums_no_line]="SHA256SUMS has no line for %s. Download both files of the same release. Nothing was changed."
    [err_sums_wrong]="%s does not match SHA256SUMS and was not used. Download it again. Nothing was changed."
    [err_self_no_line]="SHA256SUMS has no line for this script. Download both files of the same release. Nothing was changed."
    [err_self_sum]="This script does not match its line in SHA256SUMS. Download both files again. Nothing was changed."
    [err_no_download_tool]="Neither curl nor wget is installed. Put %s and SHA256SUMS next to this script and run it again. Nothing was changed."
    [err_download_failed]="Downloading %s failed. Is the internet blocked, or is there no connection? Put the file next to this script and run it again. Nothing was changed."
    [err_probe_update]="The package lists could not be renewed. Nothing was changed."
    [err_probe_failed]="The package manager cannot install the package here. Its last lines:\n%s\nNothing was changed."
    [next_probe_arch]="On Arch Linux the package databases may be old: run \"sudo pacman -Syu\" and start the script again."
    [err_no_sudo]="Administrator rights are not available here. Log in as root (su -) and run: bash %s --user %s"
    [step_package_install]="Install the Celata package (%s)"
    [step_package_update]="Update the Celata package (%s)"
    [step_package_reinstall]="Install the Celata package again to restore missing files (%s)"
    [step_package_remove]="Remove the Celata package (%s)"
    [step_dropin_write]="Tell the service where your hide.me client is (%s)"
    [step_dropin_remove]="Remove the note about the place of the client, the package fits now (%s)"
    [step_dropin_delete]="Remove the note about the place of the client (%s)"
    [step_reload]="Let the system read the changed service settings (%s)"
    [step_group_create]="Create the group celata (%s)"
    [step_group_add]="Add your account to the group celata (%s)"
    [step_service_stop]="Stop the Celata service (%s)"
    [step_state_write]="Note what this script set up (%s)"
    [step_state_delete]="Delete the note of this script (%s)"
    [step_done]="%s: done"
    [step_failed]="%s: failed: %s"
    [step_failed_item]="%s: failed"
    [step_not_run]="%s: not run"
    [step_declined]="%s: not done, you said no"
    [step_reason_default]="see the messages above"
    [err_package_unavailable]="the package file is not available"
    [err_sum_changed]="the copy of the package no longer matches SHA256SUMS, nothing was installed"
    [err_dropin_foreign]="the file does not carry the mark of this script, so it is left alone"
    [err_group_missing]="the group %s does not exist"
    [uninstall_nothing]="There is nothing to remove that this script set up."
    [uninstall_stops_service]="The service is stopped. A connection is cut and a block is lifted."
    [remains_intro]="What stays on the system:"
    [remains_group]="the group %s (remove it with: %s)"
    [remains_client]="the hide.me client and its files (Celata never installed them)"
    [remains_user_files]="your settings and cache: ~/.config/celata, ~/.cache/celata and ~/.config/autostart/celata.desktop"
    [remains_manual_unit]="the service file %s that was made by hand"
    [remains_manual_rule]="the rule %s that was made by hand"
    [remains_manual_program]="the program /usr/local/bin/celata that was made by hand"
    [remains_manual_hint]="For removing the parts made by hand see \"Removing the setup\" in https://github.com/derHouwer/celata/blob/main/docs/manual-setup.md"
    [remains_package]="the package celata (remove it with: %s)"
    [remains_dependencies]="packages that were installed together with Celata (remove them with: %s)"
)

declare -A TEXT_DE=(
    [usage_short]="Aufruf: bash celata-setup.sh [--yes] [--user NAME] [--client PFAD] | --check | --dry-run | --uninstall | --help | --version"
    [help_text]="Richtet Celata ein: findet den hide.me-Client, installiert das Celata-Paket dieser Version und nimmt dich in die Gruppe celata auf.\nEs installiert nie den Client, ändert nie das DNS, startet nie den Dienst und verbindet oder sperrt nie das Internet.\n\n  bash celata-setup.sh [--yes] [--user NAME] [--client PFAD]    einrichten oder reparieren, mit Rückfragen\n  bash celata-setup.sh --check [--user NAME] [--client PFAD]    zeigen, was vorhanden ist, nichts ändern\n  bash celata-setup.sh --dry-run [--user NAME] [--client PFAD]  zeigen, was getan würde, nichts ändern\n  bash celata-setup.sh --uninstall [--yes] [--dry-run]          entfernen, was dieses Skript eingerichtet hat\n  bash celata-setup.sh --help | --version\n\n  --yes      dem Plan und der Gruppe ohne Rückfrage zustimmen (nie dem Ersetzen einer Datei oder einem getrennten Paket)\n  --user     das Konto, für das eingerichtet wird (nötig, wenn du das Skript als root startest)\n  --client   der Ort des hide.me-Clients, wenn er an keinem bekannten Ort liegt oder es mehrere gibt"
    [version_line]="celata-setup %s-%s"
    [err_duplicate]="%s wurde zweimal angegeben"
    [err_unknown_option]="Unbekannte Option oder unbekanntes Argument: %s"
    [err_user_value]="--user braucht einen Kontonamen, zum Beispiel --user anna"
    [err_client_value]="--client braucht den vollen Pfad des Clients, zum Beispiel --client /usr/local/bin/hide.me"
    [err_combination_check]="--check lässt sich nicht mit --dry-run, --uninstall oder --yes verbinden"
    [err_combination_uninstall]="--uninstall lässt sich nicht mit --client verbinden"
    [err_bash_old]="Dieses Skript braucht bash 5.0 oder neuer (gefunden: %s)."
    [err_internal]="Interner Fehler in Zeile %s. Es wurde nichts weiter getan. Ein neuer Lauf beginnt beim jetzigen Stand."
    [aborted_by_user]="Abgebrochen. Ein neuer Lauf vervollständigt die Einrichtung."
    [choice_yes]="[J/n]"
    [choice_no]="[j/N]"
    [answer_auto_yes]="ja (--yes)"
    [err_no_terminal]="Es gibt kein Terminal für Rückfragen. Starte das Skript in einem Terminal oder füge --yes hinzu."
    [ask_user]="Du startest das als root. Für welches Konto soll Celata eingerichtet werden?"
    [err_user_needed]="Nenne dem Skript das Konto mit --user NAME. Es wurde nichts geändert."
    [err_user_invalid]="%s ist nicht brauchbar: Es muss ein vorhandenes Konto sein, das nicht root ist und eine gewöhnliche Benutzernummer hat."
    [ask_derivative]="Weitermachen?"
    [derivative_note]="%s wird als %s eingerichtet. Dieses System ist nicht geprüft."
    [dry_derivative_note]="%s würde als %s eingerichtet. Dieses System ist nicht geprüft. Ein echter Lauf fragt vorher."
    [ask_execute]="Jetzt ausführen?"
    [client_choose_intro]="Es wurden mehrere hide.me-Clients gefunden:"
    [ask_client_number]="Welchen soll Celata nutzen? Gib eine Zahl von 1 bis %s ein:"
    [err_client_choice]="Das ist keine gültige Wahl. Es wurde nichts geändert."
    [ask_install_beside_manual]="%s: Celata ist auf diesem System von Hand eingerichtet, und das Paket bringt eine eigene Dienst-Datei und Regel mit. Trotzdem tun?"
    [ask_update_preexisting]="%s: Das Paket war schon vor diesem Skript installiert. Aktualisieren?"
    [ask_reinstall_preexisting]="%s: Das Paket war schon vor diesem Skript installiert. Tun?"
    [ask_remove_preexisting]="%s: Das Paket war schon vor diesem Skript installiert. Entfernen?"
    [ask_dropin_replace]="%s: Eine Datei dieses Namens gibt es schon. Ersetzen?"
    [ask_dropin_remove]="%s: Die Datei wird nicht mehr gebraucht. Entfernen?"
    [ask_step_generic]="%s: tun?"
    [plan_asks_separately]="(fragt einzeln)"
    [header]="Celata %s – Einrichtung"
    [header_system]="System: %s"
    [all_set]="Alles ist eingerichtet. Es gibt nichts zu tun."
    [plan_intro]="Das wird getan:"
    [plan_sudo_note]="Schritte mit Administratorrechten nutzen sudo, ein Befehl nach dem anderen. Es kann nach deinem Passwort fragen."
    [preflight_announce]="Ich prüfe jetzt, ob alles bereitsteht: %s. Es wird noch nichts installiert."
    [preflight_sudo_note]="sudo fragt jetzt eventuell nach deinem Passwort."
    [preflight_downloading]="Lade %s herunter ..."
    [part_lists]="die Paketlisten erneuern (braucht sudo)"
    [part_download]="das Celata-Paket herunterladen"
    [part_probe]="das Paket ausprobieren, ohne es zu installieren"
    [part_rule]="%s lesen (braucht sudo)"
    [dry_would_download]="würde herunterladen: %s"
    [dry_would_run]="würde ausführen: %s"
    [dry_package_checked]="Paketdatei %s liegt neben dem Skript und passt zu SHA256SUMS"
    [summary_done]="Erledigt:"
    [summary_open]="Offen:"
    [summary_notes]="Hinweise:"
    [summary_nothing_missing]="Nichts fehlt."
    [summary_relogin]="Melde dich einmal ab und wieder an, damit die Gruppe wirkt."
    [summary_restart]="Die geänderte Dienst-Datei gilt erst nach einem Neustart des Dienstes. Wenn du nicht verbunden bist, führe aus: sudo systemctl restart %s"
    [chk_name_system]="System"
    [chk_name_blocked]="Internet"
    [chk_name_client]="hide.me-Client"
    [chk_name_client_version]="Version des Clients"
    [chk_name_token]="Access-Token"
    [chk_name_package]="Celata-Paket"
    [chk_name_unit]="Dienst-Datei"
    [chk_name_group]="Gruppe"
    [chk_name_member]="Mitgliedschaft"
    [chk_name_session]="Sitzung"
    [chk_name_polkit]="Regel für polkit"
    [chk_name_dns]="DNS"
    [chk_name_tray]="Leiste für Symbole"
    [chk_name_other_service]="Anderer hide.me-Dienst"
    [chk_system_tested]="%s (geprüft)"
    [chk_system_derivative]="%s als %s, nicht geprüft"
    [label_as]="%s als %s"
    [reason_architecture]="%s ist kein x86_64-System"
    [reason_no-systemd]="%s läuft nicht mit systemd"
    [reason_atomic]="%s ist ein Abbild-System (atomic), das Paket lässt sich dort nicht installieren"
    [reason_opensuse]="%s (openSUSE) wird noch nicht unterstützt"
    [reason_old-release]="diese Fassung von %s wird nicht unterstützt"
    [reason_unknown]="%s ist kein System, das dieses Skript kennt"
    [next_unsupported]="Unterstützt: Fedora 44 und 45, Debian 13, Ubuntu 24.04, 26.04 und 26.10, Linux Mint 22 und Arch Linux. Ableger davon mit klarer Grundlage lassen sich einrichten, sind aber nicht geprüft. Einrichtung von Hand: https://github.com/derHouwer/celata/blob/main/docs/manual-setup.md"
    [chk_blocked_no]="nicht gesperrt"
    [chk_blocked_yes]="Internet gesperrt (die Internet-Sperre des Dienstes ist an)"
    [block_exit]="Zum Aufheben: Klicke in Celata auf \"Sperre aufheben\", führe \"celata --unblock\" aus oder führe \"sudo systemctl stop %s\" aus."
    [err_blocked]="Das Internet ist gesperrt, deshalb wird nichts heruntergeladen oder installiert. Es wurde nichts geändert.\n%s"
    [chk_client_ok]="%s (Arbeitsordner %s)"
    [chk_client_missing]="nicht gefunden (gesucht in /opt/hide.me, /usr/bin und /usr/local/bin)"
    [chk_client_incomplete]="%s hat keine CA.pem daneben, der Client wirkt unvollständig"
    [chk_client_ambiguous]="mehrere gefunden: %s"
    [chk_client_unsafe_path]="%s enthält Zeichen, die das Skript nicht verarbeitet (erlaubt sind nur Buchstaben, Ziffern und . _ + - /)"
    [chk_client_untrusted]="unsicherer Ort: %s gehört nicht root oder ist für andere beschreibbar"
    [next_client_missing]="Installiere zuerst den hide.me-Client, wie unter https://github.com/eventure/hide.client.linux beschrieben. Das Skript installiert ihn nicht."
    [next_client_ambiguous]="Starte das Skript erneut mit --client PFAD."
    [next_client_untrusted]="Prüfe, wer es geändert hat. Wenn es deins ist: sudo chown -R root:root %s und sudo chmod -R go-w %s"
    [err_client_ambiguous]="Es wurden mehrere hide.me-Clients gefunden (%s). Wähle einen mit --client PFAD. Es wurde nichts geändert."
    [chk_version_ok]="%s"
    [chk_version_other]="%s, Celata ist aber mit %s geprüft"
    [chk_version_unknown]="erst bei laufendem Dienst sichtbar"
    [chk_token_ok]="vorhanden"
    [chk_token_missing]="kein Access-Token gefunden"
    [chk_token_unknown]="nicht geprüft (kein Client)"
    [next_token]="Lege einen an, wie in der Anleitung von hide.me beschrieben (https://github.com/eventure/hide.client.linux), Befehl \"hide.me token\"."
    [chk_package_none]="nicht installiert"
    [chk_package_nosystem]="nicht geprüft (System nicht unterstützt)"
    [chk_package_manual]="nicht als Paket installiert, Celata ist von Hand eingerichtet"
    [chk_package_old]="Version %s ist älter als dieses Skript (%s)"
    [chk_package_ok_setup]="Version %s, von diesem Skript installiert"
    [chk_package_ok_preexisting]="Version %s, schon vor diesem Skript installiert"
    [chk_unit_ok]="passt zum Client"
    [chk_unit_absent]="noch keine Dienst-Datei (sie kommt mit dem Paket)"
    [chk_unit_noclient]="nicht geprüft (kein Client)"
    [chk_unit_needs_dropin]="der Client liegt nicht dort, wo das Paket ihn erwartet (%s), eine Ergänzung behebt das"
    [chk_unit_differs]="die Dienst-Datei passt nicht zum Client (%s)"
    [next_unit_differs]="Das Skript ändert sie nicht. Genutzte Ergänzungen: %s"
    [chk_group_ok]="Gruppe %s ist vorhanden"
    [chk_group_missing]="Gruppe %s gibt es noch nicht"
    [chk_member_ok]="%s ist Mitglied"
    [chk_member_missing]="%s ist nicht Mitglied der Gruppe %s"
    [chk_session_ok]="die Gruppe ist in dieser Sitzung wirksam"
    [chk_session_relogin]="melde dich ab und wieder an, damit die Gruppe wirkt"
    [chk_session_nomember]="nicht geprüft, du bist noch nicht Mitglied"
    [chk_session_root]="als root nicht prüfbar"
    [chk_polkit_package]="Regel des Pakets vorhanden"
    [chk_polkit_manual]="Regel unter /etc vorhanden (von Hand)"
    [chk_polkit_absent]="keine Regel gefunden (sie kommt mit dem Paket)"
    [chk_polkit_lost]="die Regel des Pakets fehlt"
    [chk_polkit_differs]="die Regel unter /etc weicht von der von Celata ab"
    [chk_polkit_unknown]="ohne Administratorrechte nicht prüfbar"
    [chk_polkit_old]="polkit %s ist zu alt (ab %s)"
    [next_polkit_differs]="Das Skript ändert eine Einrichtung von Hand nicht."
    [next_polkit_unknown]="Führe aus: sudo bash %s --check"
    [err_polkit_old]="polkit %s ist zu alt für die Regel von Celata (nötig ist %s oder neuer). Es wurde nichts geändert."
    [chk_dns_none]="kein systemd-resolved aktiv"
    [chk_dns_leak]="systemd-resolved läuft, DNS-Anfragen können am VPN vorbeigehen"
    [next_dns]="Celata zeigt auch einen Hinweis, siehe https://github.com/derHouwer/celata/blob/main/docs/dns-leak.md. Das Skript ändert hier nichts."
    [chk_tray_ok]="gefunden"
    [chk_tray_missing]="keine gefunden, das Symbol wird fehlen, Celata läuft trotzdem (GNOME braucht eine Erweiterung)"
    [chk_tray_unknown]="nicht erkennbar (kein Sitzungsbus)"
    [chk_other_none]="kein anderer hide.me-Dienst aktiv"
    [chk_other_service]="%s läuft"
    [next_other_service]="Beide nutzen dieselbe Schnittstelle. Zum Stoppen: sudo systemctl stop %s"
    [err_sums_missing]="SHA256SUMS liegt nicht neben diesem Skript, deshalb lässt sich %s nicht prüfen. Lade SHA256SUMS derselben Fassung in diesen Ordner. Es wurde nichts geändert."
    [err_sums_no_line]="SHA256SUMS hat keine Zeile für %s. Lade beide Dateien derselben Fassung herunter. Es wurde nichts geändert."
    [err_sums_wrong]="%s passt nicht zu SHA256SUMS und wurde nicht benutzt. Lade sie erneut herunter. Es wurde nichts geändert."
    [err_self_no_line]="SHA256SUMS hat keine Zeile für dieses Skript. Lade beide Dateien derselben Fassung herunter. Es wurde nichts geändert."
    [err_self_sum]="Dieses Skript passt nicht zu seiner Zeile in SHA256SUMS. Lade beide Dateien erneut herunter. Es wurde nichts geändert."
    [err_no_download_tool]="Weder curl noch wget ist installiert. Lege %s und SHA256SUMS neben dieses Skript und starte es erneut. Es wurde nichts geändert."
    [err_download_failed]="Das Herunterladen von %s ist gescheitert. Ist das Internet gesperrt oder fehlt die Verbindung? Lege die Datei neben dieses Skript und starte es erneut. Es wurde nichts geändert."
    [err_probe_update]="Die Paketlisten ließen sich nicht erneuern. Es wurde nichts geändert."
    [err_probe_failed]="Der Paketmanager kann das Paket hier nicht installieren. Seine letzten Zeilen:\n%s\nEs wurde nichts geändert."
    [next_probe_arch]="Unter Arch Linux können die Paketdatenbanken veraltet sein: Führe \"sudo pacman -Syu\" aus und starte das Skript erneut."
    [err_no_sudo]="Administratorrechte sind hier nicht verfügbar. Melde dich als root an (su -) und führe aus: bash %s --user %s"
    [step_package_install]="Das Celata-Paket installieren (%s)"
    [step_package_update]="Das Celata-Paket aktualisieren (%s)"
    [step_package_reinstall]="Das Celata-Paket erneut installieren, um fehlende Dateien herzustellen (%s)"
    [step_package_remove]="Das Celata-Paket entfernen (%s)"
    [step_dropin_write]="Dem Dienst sagen, wo dein hide.me-Client liegt (%s)"
    [step_dropin_remove]="Den Hinweis auf den Ort des Clients entfernen, das Paket passt jetzt (%s)"
    [step_dropin_delete]="Den Hinweis auf den Ort des Clients entfernen (%s)"
    [step_reload]="Das System die geänderten Dienst-Einstellungen lesen lassen (%s)"
    [step_group_create]="Die Gruppe celata anlegen (%s)"
    [step_group_add]="Dein Konto zur Gruppe celata hinzufügen (%s)"
    [step_service_stop]="Den Celata-Dienst stoppen (%s)"
    [step_state_write]="Festhalten, was dieses Skript eingerichtet hat (%s)"
    [step_state_delete]="Die Notiz dieses Skripts löschen (%s)"
    [step_done]="%s: erledigt"
    [step_failed]="%s: gescheitert: %s"
    [step_failed_item]="%s: gescheitert"
    [step_not_run]="%s: nicht ausgeführt"
    [step_declined]="%s: nicht getan, du hast Nein gesagt"
    [step_reason_default]="siehe die Meldungen darüber"
    [err_package_unavailable]="die Paketdatei steht nicht bereit"
    [err_sum_changed]="die Kopie des Pakets passt nicht mehr zu SHA256SUMS, es wurde nichts installiert"
    [err_dropin_foreign]="die Datei trägt nicht die Kennung dieses Skripts und bleibt deshalb unberührt"
    [err_group_missing]="die Gruppe %s gibt es nicht"
    [uninstall_nothing]="Es gibt nichts zu entfernen, was dieses Skript eingerichtet hat."
    [uninstall_stops_service]="Der Dienst wird gestoppt. Eine Verbindung wird getrennt, eine Sperre aufgehoben."
    [remains_intro]="Was auf dem System bleibt:"
    [remains_group]="die Gruppe %s (entfernen mit: %s)"
    [remains_client]="der hide.me-Client und seine Dateien (Celata hat sie nie installiert)"
    [remains_user_files]="deine Einstellungen und der Zwischenspeicher: ~/.config/celata, ~/.cache/celata und ~/.config/autostart/celata.desktop"
    [remains_manual_unit]="die von Hand angelegte Dienst-Datei %s"
    [remains_manual_rule]="die von Hand angelegte Regel %s"
    [remains_manual_program]="das von Hand angelegte Programm /usr/local/bin/celata"
    [remains_manual_hint]="Zum Entfernen der Teile von Hand siehe \"Removing the setup\" in https://github.com/derHouwer/celata/blob/main/docs/manual-setup.md"
    [remains_package]="das Paket celata (entfernen mit: %s)"
    [remains_dependencies]="Pakete, die zusammen mit Celata installiert wurden (entfernen mit: %s)"
)
# END TEXTS

# --- Output ----------------------------------------------------------------------------------------------

# Text of a message in the current language; falls back to English, then to the key.
text_of() {
    local key=$1
    if [[ $LANG_CODE == de && -n ${TEXT_DE[$key]+x} ]]; then
        printf '%s' "${TEXT_DE[$key]}"
    elif [[ -n ${TEXT_EN[$key]+x} ]]; then
        printf '%s' "${TEXT_EN[$key]}"
    else
        printf '%s' "$key"
    fi
}

# Message without a line break. The text is the format and the values are only ever arguments.
fmt() {
    local key=$1 format
    shift
    format=$(text_of "$key")
    # shellcheck disable=SC2059 # the message text is the format on purpose
    printf -- "$format" "$@"
}

say() {
    fmt "$@"
    printf '\n'
}

say_err() {
    fmt "$@" >&2
    printf '\n' >&2
}

# Prints an error and ends the run with the given return value.
die() {
    local rc=$1
    shift
    say_err "$@"
    exit "$rc"
}

detect_language() {
    local name value=''
    for name in LC_ALL LC_MESSAGES LANG; do
        value=${!name:-}
        if [[ -n $value ]]; then
            break
        fi
    done
    case $value in
        de*) LANG_CODE=de ;;
        *) LANG_CODE=en ;;
    esac
}

use_utf8() {
    UTF8=0
    if [[ $(locale charmap 2>/dev/null || true) == UTF-8 ]]; then
        UTF8=1
    fi
}

# Mark for a result: ok, fail, hint or unknown.
mark() {
    case $1 in
        ok) if ((UTF8)); then printf '✓'; else printf '[OK]'; fi ;;
        fail) if ((UTF8)); then printf '✗'; else printf '[--]'; fi ;;
        hint) printf '!' ;;
        *) printf '?' ;;
    esac
}

join_by() {
    local separator=$1 first=1 item
    shift
    for item in "$@"; do
        if ((first)); then
            printf '%s' "$item"
            first=0
        else
            printf '%s%s' "$separator" "$item"
        fi
    done
}

# --- Command line ----------------------------------------------------------------------------------------

usage_error() {
    say_err "$@"
    say_err usage_short
    return "$RC_USAGE"
}

# Sets the flag variable named by $1 and refuses a second use of the option $2.
set_flag() {
    local -n flag=$1
    if ((flag)); then
        usage_error err_duplicate "$2"
        return 1
    fi
    flag=1
}

parse_args() {
    # shellcheck disable=SC2034 # set through a name reference in set_flag
    local seen_user=0 seen_client=0
    while (("$#")); do
        case $1 in
            --yes) set_flag OPT_YES --yes || return "$RC_USAGE" ;;
            --check) set_flag OPT_CHECK --check || return "$RC_USAGE" ;;
            --dry-run) set_flag OPT_DRY --dry-run || return "$RC_USAGE" ;;
            --uninstall) set_flag OPT_UNINSTALL --uninstall || return "$RC_USAGE" ;;
            --help) set_flag OPT_HELP --help || return "$RC_USAGE" ;;
            --version) set_flag OPT_VERSION --version || return "$RC_USAGE" ;;
            --user)
                set_flag seen_user --user || return "$RC_USAGE"
                if (("$#" < 2)) || [[ ! $2 =~ ^[A-Za-z_][A-Za-z0-9_.-]{0,31}$ ]]; then
                    usage_error err_user_value
                    return "$RC_USAGE"
                fi
                OPT_USER=$2
                shift
                ;;
            --client)
                set_flag seen_client --client || return "$RC_USAGE"
                if (("$#" < 2)) || [[ $2 != /* ]]; then
                    usage_error err_client_value
                    return "$RC_USAGE"
                fi
                OPT_CLIENT=$2
                shift
                ;;
            *)
                usage_error err_unknown_option "$1"
                return "$RC_USAGE"
                ;;
        esac
        shift
    done
    if ((OPT_CHECK)) && ((OPT_DRY || OPT_UNINSTALL || OPT_YES)); then
        usage_error err_combination_check
        return "$RC_USAGE"
    fi
    if ((OPT_UNINSTALL)) && [[ -n $OPT_CLIENT ]]; then
        usage_error err_combination_uninstall
        return "$RC_USAGE"
    fi
    return 0
}

# --- Rights, questions, clean-up -------------------------------------------------------------------------

# Runs one command with administrator rights: directly as root, otherwise through sudo, one command at a time.
as_root() {
    if ((OPT_DRY)); then
        printf 'sudo'
        printf ' %q' "$@"
        printf '\n'
        return 0
    fi
    if ((EUID == 0)); then
        "$@"
    else
        sudo -- "$@"
    fi
}

sudo_available() {
    if ((EUID == 0)); then
        return 0
    fi
    command -v sudo >/dev/null 2>&1 || return 1
    if sudo -n true 2>/dev/null; then
        return 0
    fi
    [[ -t 0 ]]
}

# Is the answer a yes ("y", "yes", "j", "ja", any case)?
is_yes() {
    local answer=${1,,}
    answer=${answer//[[:space:]]/}
    [[ $answer == y || $answer == yes || $answer == j || $answer == ja ]]
}

# ask KEY DEFAULT CONSENT [values...]: DEFAULT is yes or no; CONSENT is plan, account, overwrite or separate.
# Returns 0 for yes and 1 for no. --yes answers "plan" and "account" only.
ask() {
    local key=$1 default=$2 consent=$3 answer=''
    shift 3
    if ((OPT_YES)) && [[ $consent == plan || $consent == account ]]; then
        fmt "$key" "$@"
        printf ' %s\n' "$(fmt answer_auto_yes)"
        return 0
    fi
    fmt "$key" "$@"
    printf ' %s ' "$(fmt "choice_$default")"
    if ! IFS= read -r answer; then
        printf '\n'
        if ((OPT_YES)); then
            return 1
        fi
        die "$RC_ABORT_CLEAN" err_no_terminal
    fi
    if [[ -z ${answer//[[:space:]]/} ]]; then
        [[ $default == yes ]]
        return
    fi
    is_yes "$answer"
}

make_workdir() {
    WORKDIR=$(mktemp -d "${TMPDIR:-/tmp}/celata-setup.XXXXXXXX")
    chmod 0755 -- "$WORKDIR"
}

# Creates the folder for the package copy that only root can change (see step_package).
make_root_pkgdir() {
    PKGDIR=$(as_root mktemp -d -p /var/tmp celata-setup.XXXXXXXX)
    as_root chmod 0755 -- "$PKGDIR"
}

# Removes the folder of the package copy; only a folder this script made.
remove_root_pkgdir() {
    if [[ $PKGDIR == /var/tmp/celata-setup.?* ]]; then
        if ! as_root rm -rf -- "$PKGDIR" 2>/dev/null; then
            sudo -n -- rm -rf -- "$PKGDIR" 2>/dev/null || true
        fi
    fi
    PKGDIR=''
}

on_exit() {
    local rc=$?
    trap - EXIT
    if [[ -n $WORKDIR && -d $WORKDIR ]]; then
        rm -rf -- "$WORKDIR"
    fi
    if [[ -n $PKGDIR ]]; then
        remove_root_pkgdir
    fi
    exit "$rc"
}

on_signal() {
    printf '\n'
    say aborted_by_user
    exit "$RC_USER_ABORT"
}

# Writes the standard input to $1 with mode $2: first under a temporary name next to it, then renamed.
write_atomic() {
    local target=$1 mode=$2 temporary
    temporary=$(mktemp "$target.XXXXXXXX")
    cat >"$temporary"
    chmod "$mode" -- "$temporary"
    mv -f -- "$temporary" "$target"
}

# Puts the file $1 in place as $2 with mode $3, as root: copy next to the target, then rename.
root_put_atomic() {
    local from=$1 target=$2 mode=$3
    as_root install -m "$mode" -- "$from" "$target.new"
    as_root mv -f -- "$target.new" "$target"
}

# The user the script sets up Celata for: --user, SUDO_USER, PKEXEC_UID, the own account, or a question.
resolve_user() {
    local name=''
    if [[ -n $OPT_USER ]]; then
        name=$OPT_USER
    elif [[ -n ${SUDO_USER:-} && $SUDO_USER != root ]]; then
        name=$SUDO_USER
    elif [[ -n ${PKEXEC_UID:-} ]]; then
        name=$(getent passwd "$PKEXEC_UID" | cut -d: -f1) || name=''
    elif ((EUID != 0)); then
        name=$(id -un)
    else
        fmt ask_user
        printf ' '
        if ! IFS= read -r name; then
            printf '\n'
            die "$RC_ABORT_CLEAN" err_user_needed
        fi
    fi
    if ! user_acceptable "$name"; then
        die "$RC_ABORT_CLEAN" err_user_invalid "$name"
    fi
    USER_NAME=$name
}

uid_acceptable() {
    [[ $1 =~ ^[0-9]+$ ]] && (($1 >= MIN_UID && $1 < MAX_UID))
}

# An account that exists, is not root and has a normal user number.
user_acceptable() {
    local name=$1 uid
    [[ -n $name && $name != root ]] || return 1
    uid=$(id -u -- "$name" 2>/dev/null) || return 1
    uid_acceptable "$uid"
}

# --- System: reading and classifying -----------------------------------------------------------------------

# Value of a line of os-release without its quotes. The text is never run or expanded.
unquote() {
    local value=$1
    if [[ $value == \"*\" && ${#value} -ge 2 ]] || [[ $value == \'*\' && ${#value} -ge 2 ]]; then
        value=${value:1:${#value}-2}
    fi
    printf '%s' "$value"
}

# Reads an os-release file as plain text into the OS_* variables.
read_os_release() {
    local file=$1 line key value
    OS_ID='' OS_LIKE='' OS_VERSION='' OS_CODENAME='' OS_UBUNTU_CODENAME='' OS_DEBIAN_CODENAME=''
    OS_PLATFORM='' OS_VARIANT='' OS_PRETTY=''
    while IFS= read -r line || [[ -n $line ]]; do
        [[ $line =~ ^([A-Z_]+)=(.*)$ ]] || continue
        key=${BASH_REMATCH[1]}
        value=$(unquote "${BASH_REMATCH[2]}")
        case $key in
            ID) OS_ID=$value ;;
            ID_LIKE) OS_LIKE=$value ;;
            VERSION_ID) OS_VERSION=$value ;;
            VERSION_CODENAME) OS_CODENAME=$value ;;
            UBUNTU_CODENAME) OS_UBUNTU_CODENAME=$value ;;
            DEBIAN_CODENAME) OS_DEBIAN_CODENAME=$value ;;
            PLATFORM_ID) OS_PLATFORM=$value ;;
            VARIANT_ID) OS_VARIANT=$value ;;
            PRETTY_NAME) OS_PRETTY=$value ;;
        esac
    done <"$file"
}

os_release_file() {
    local file
    for file in /etc/os-release /usr/lib/os-release; do
        if [[ -r $file ]]; then
            printf '%s' "$file"
            return 0
        fi
    done
    return 1
}

# Facts about the running system that are not in os-release.
gather_system_facts() {
    MACHINE=$(uname -m)
    HAS_SYSTEMD=0
    if [[ -d /run/systemd/system ]]; then
        HAS_SYSTEMD=1
    fi
    IS_OSTREE=0
    if [[ -e /run/ostree-booted ]]; then
        IS_OSTREE=1
    fi
}

like_has() {
    local word IFS=' '
    for word in $OS_LIKE; do
        if [[ $word == "$1" ]]; then
            return 0
        fi
    done
    return 1
}

# Version for a codename of a family, empty if unknown.
codename_version() {
    case $1:$2 in
        ubuntu:noble) printf '24.04' ;;
        ubuntu:resolute) printf '26.04' ;;
        ubuntu:stonking) printf '26.10' ;;
        ubuntu:jammy) printf '22.04' ;;
        ubuntu:focal) printf '20.04' ;;
        debian:trixie) printf '13' ;;
        debian:bookworm) printf '12' ;;
        debian:bullseye) printf '11' ;;
    esac
}

# Sets CL_BASE, CL_BASE_VERSION, CL_FAMILY, CL_KIND (tested, derivative, unsupported) and CL_REASON from the
# OS_* variables and the facts of the system. The order of the exclusions is part of the behaviour.
classify() {
    local version='' codename=''
    CL_BASE='' CL_BASE_VERSION='' CL_FAMILY='' CL_KIND=unsupported CL_REASON=unknown
    if [[ $MACHINE != x86_64 ]]; then
        CL_REASON=architecture
        return 0
    fi
    if ((HAS_SYSTEMD != 1)); then
        CL_REASON=no-systemd
        return 0
    fi
    case $OS_VARIANT:$OS_ID in
        silverblue:* | kinoite:* | sericea:* | onyx:* | cosmic-atomic:* | *:bazzite | *:bluefin | *:aurora)
            CL_REASON=atomic
            return 0
            ;;
    esac
    if ((IS_OSTREE == 1)); then
        CL_REASON=atomic
        return 0
    fi
    case $OS_ID in
        opensuse* | sles | sled | suse)
            CL_REASON=opensuse
            return 0
            ;;
    esac

    if [[ $OS_ID == ubuntu || -n $OS_UBUNTU_CODENAME ]] || like_has ubuntu; then
        CL_BASE=ubuntu CL_FAMILY=deb
        if [[ $OS_ID == ubuntu ]]; then
            version=$OS_VERSION
        else
            version=$(codename_version ubuntu "$OS_UBUNTU_CODENAME")
        fi
        case $version in
            24.04 | 26.04 | 26.10) CL_BASE_VERSION=$version ;;
            *)
                CL_BASE_VERSION=$version
                CL_REASON=unknown
                if [[ -n $version ]]; then
                    CL_REASON=old-release
                fi
                return 0
                ;;
        esac
        CL_KIND=derivative
        if [[ $OS_ID == ubuntu ]]; then
            CL_KIND=tested
        elif [[ $OS_ID == linuxmint && $OS_UBUNTU_CODENAME == noble && $OS_VERSION == 22* ]]; then
            CL_KIND=tested
        fi
        CL_REASON=''
        return 0
    fi

    if [[ $OS_ID == debian ]] || like_has debian; then
        CL_BASE=debian CL_FAMILY=deb
        if [[ $OS_ID == debian ]]; then
            version=$OS_VERSION
        else
            codename=${OS_DEBIAN_CODENAME:-$OS_CODENAME}
            version=$(codename_version debian "$codename")
        fi
        CL_BASE_VERSION=$version
        if [[ $version == 13 ]]; then
            CL_KIND=derivative
            if [[ $OS_ID == debian ]]; then
                CL_KIND=tested
            fi
            CL_REASON=''
            return 0
        fi
        CL_REASON=unknown
        if [[ -n $version ]]; then
            CL_REASON=old-release
        fi
        return 0
    fi

    if [[ $OS_ID == fedora ]] || { like_has fedora && [[ $OS_PLATFORM == platform:f[0-9]* ]]; }; then
        CL_BASE=fedora CL_FAMILY=rpm
        if [[ $OS_ID == fedora ]]; then
            version=$OS_VERSION
        else
            version=${OS_PLATFORM#platform:f}
        fi
        CL_BASE_VERSION=$version
        case $version in
            44 | 45)
                CL_KIND=derivative
                if [[ $OS_ID == fedora ]]; then
                    CL_KIND=tested
                fi
                CL_REASON=''
                ;;
            *)
                CL_REASON=unknown
                if [[ $version =~ ^[0-9]+$ ]]; then
                    CL_REASON=old-release
                fi
                ;;
        esac
        return 0
    fi

    if [[ $OS_ID == arch ]] || like_has arch; then
        CL_BASE=arch CL_FAMILY=arch
        CL_KIND=derivative
        if [[ $OS_ID == arch ]]; then
            CL_KIND=tested
        fi
        CL_REASON=''
        return 0
    fi
    CL_REASON=unknown
}

base_title() {
    case $1 in
        fedora) printf 'Fedora' ;;
        debian) printf 'Debian' ;;
        ubuntu) printf 'Ubuntu' ;;
        arch) printf 'Arch Linux' ;;
    esac
}

# Short name of the base: "Fedora 44", "Arch Linux".
base_label() {
    if [[ -n $CL_BASE_VERSION ]]; then
        printf '%s %s' "$(base_title "$CL_BASE")" "$CL_BASE_VERSION"
    else
        base_title "$CL_BASE"
    fi
}

# Name of the package file for the family of the system.
package_file() {
    case ${1:-$CL_FAMILY} in
        deb) printf 'celata_%s-%s_amd64.deb' "$CELATA_VERSION" "$CELATA_PKGREL" ;;
        rpm) printf 'celata-%s-%s.x86_64.rpm' "$CELATA_VERSION" "$CELATA_PKGREL" ;;
        arch) printf 'celata-%s-%s-x86_64.pkg.tar.zst' "$CELATA_VERSION" "$CELATA_PKGREL" ;;
    esac
}

# Is version $1 at least $2? Compares like "sort -V".
version_at_least() {
    local lowest
    lowest=$(printf '%s\n%s\n' "$1" "$2" | LC_ALL=C sort -V | head -n 1)
    [[ $lowest == "$2" ]]
}

# Is the installed polkit new enough for rules in JavaScript? Not installed counts as fine: the package
# brings it. Prints the version found.
polkit_ok() {
    local output version
    command -v pkaction >/dev/null 2>&1 || return 0
    output=$(LC_ALL=C pkaction --version 2>/dev/null) || return 0
    version=${output##* }
    [[ $version =~ ^[0-9]+(\.[0-9]+)*$ ]] || return 0
    printf '%s' "$version"
    version_at_least "$version" "$MIN_POLKIT"
}

# --- Package manager ---------------------------------------------------------------------------------------

# Sets PM_ARGV to the command of an action for the family: probe-update, probe, install, update, reinstall,
# remove. FILE is the package file for all but probe-update and remove.
pm_build() {
    local action=$1 file=${2:-}
    PM_ARGV=()
    case $CL_FAMILY:$action in
        deb:probe-update) PM_ARGV=(apt-get update) ;;
        deb:probe) PM_ARGV=(apt-get install --simulate "$file") ;;
        deb:install | deb:update) PM_ARGV=(env DEBIAN_FRONTEND=noninteractive apt-get install -y "$file") ;;
        deb:reinstall) PM_ARGV=(env DEBIAN_FRONTEND=noninteractive apt-get install -y --reinstall "$file") ;;
        deb:remove) PM_ARGV=(env DEBIAN_FRONTEND=noninteractive apt-get remove -y celata) ;;
        rpm:probe) PM_ARGV=(dnf install --assumeno "$file") ;;
        rpm:install | rpm:update) PM_ARGV=(dnf install -y "$file") ;;
        rpm:reinstall) PM_ARGV=(dnf reinstall -y "$file") ;;
        rpm:remove) PM_ARGV=(dnf remove -y celata) ;;
        arch:probe) PM_ARGV=(pacman -U --print "$file") ;;
        arch:install | arch:update) PM_ARGV=(pacman -U --noconfirm --needed "$file") ;;
        arch:reinstall) PM_ARGV=(pacman -U --noconfirm "$file") ;;
        arch:remove) PM_ARGV=(pacman -R --noconfirm celata) ;;
        *) return 1 ;;
    esac
}

# Version of an installed package from the output of the query of the family; fails if not installed.
parse_package_version() {
    local family=$1 output=$2 version
    case $family in
        deb)
            [[ $output == 'install ok installed '* ]] || return 1
            version=${output#install ok installed }
            ;;
        rpm)
            [[ -n $output && $output != *'not installed'* ]] || return 1
            version=$output
            ;;
        arch)
            [[ $output == celata\ * ]] || return 1
            version=${output#celata }
            ;;
        *) return 1 ;;
    esac
    [[ -n $version && $version != *' '* ]] || return 1
    printf '%s' "$version"
}

# Version of the installed package "celata"; fails if there is none.
installed_package_version() {
    local output=''
    # shellcheck disable=SC2016 # dpkg-query expands the fields itself
    local deb_format='${Status} ${Version}'
    case $CL_FAMILY in
        deb) output=$(LC_ALL=C dpkg-query -W -f "$deb_format" celata 2>/dev/null) || return 1 ;;
        rpm) output=$(LC_ALL=C rpm -q --qf '%{VERSION}-%{RELEASE}' celata 2>/dev/null) || return 1 ;;
        arch) output=$(LC_ALL=C pacman -Q celata 2>/dev/null) || return 1 ;;
        *) return 1 ;;
    esac
    parse_package_version "$CL_FAMILY" "$output"
}

# Judges a probe of the package manager from its return value and output; 0 means all dependencies can be met.
probe_succeeded() {
    local rc=$1 output=$2
    case $CL_FAMILY in
        deb | arch) ((rc == 0)) ;;
        rpm) ((rc == 0)) || { ((rc == 1)) && [[ $output == *'Operation aborted by the user'* ]]; } ;;
        *) return 1 ;;
    esac
}

# Are the package lists of a Debian-like system fresh (changed within a day)?
apt_lists_fresh() {
    local directory=${1:-/var/lib/apt/lists} found
    found=$(find "$directory" -maxdepth 1 -type f -name '*Packages*' -mtime -1 2>/dev/null | head -n 1) || found=''
    [[ -n $found ]]
}

# --- Client -----------------------------------------------------------------------------------------------

# Prints the places (one per line) that hold an executable regular file, without doubles; the places come
# as arguments.
find_clients() {
    local place real seen=$'\n'
    for place in "$@"; do
        [[ -f $place && -x $place ]] || continue
        real=$(realpath -e -- "$place" 2>/dev/null) || continue
        [[ -f $real && -x $real ]] || continue
        if [[ $seen == *$'\n'"$real"$'\n'* ]]; then
            continue
        fi
        seen+="$real"$'\n'
        printf '%s\n' "$real"
    done
}

# Working folder for a client: the first folder with CA.pem among the folder of the client and the arguments.
client_dir() {
    local client=$1 folder
    shift
    for folder in "$(dirname -- "$client")" "$@"; do
        if [[ -f $folder/CA.pem ]]; then
            realpath -e -- "$folder"
            return 0
        fi
    done
    return 1
}

# Client and working folder the package of the family expects.
expected_client() {
    case $1 in
        deb | rpm) printf '%s %s' /opt/hide.me/hide.me /opt/hide.me ;;
        arch) printf '%s %s' /usr/bin/hide.me /etc/hide.me ;;
    esac
}

# Is a path safe to write into a unit file (no spaces, no characters systemd treats specially)?
plain_path() {
    [[ $1 =~ ^/[A-Za-z0-9_./+-]+$ ]]
}

# Judges lines "<path> <uid> <mode>" (one per file and folder up to /): all must belong to root and nobody
# but root may write to them. Prints the first path that fails.
chain_trusted() {
    local line mode rest uid path
    while IFS= read -r line; do
        [[ -n $line ]] || continue
        mode=${line##* }
        rest=${line% *}
        uid=${rest##* }
        path=${rest% *}
        if [[ $uid != 0 ]] || ! [[ $mode =~ ^[0-7]+$ ]] || ((8#$mode & 8#022)); then
            printf '%s' "$path"
            return 1
        fi
    done
    return 0
}

# Lines "<path> <uid> <mode>" for a file and every folder above it, after resolving links.
stat_chain() {
    local path
    path=$(realpath -e -- "$1") || return 1
    while :; do
        LC_ALL=C stat -c '%n %u %a' -- "$path"
        [[ $path == / ]] && break
        path=$(dirname -- "$path")
    done
}

# Is the client at a place only root can change? Looks at the client and at CA.pem. Prints the first bad path.
client_trusted() {
    local client=$1 folder=$2 bad
    bad=$( { stat_chain "$client"; stat_chain "$folder/CA.pem"; } | chain_trusted) || {
        printf '%s' "$bad"
        return 1
    }
    return 0
}

# Does a plain "[ -s file ]" test find an Access-Token file in the folder? The file is never opened.
token_present() {
    [ -s "$1/accessToken.txt" ]
}

# Version of the client from the running service; fails if it cannot be asked.
service_client_version() {
    local output
    command -v curl >/dev/null 2>&1 || return 1
    [[ -S $CONTROL_SOCKET && -w $CONTROL_SOCKET ]] || return 1
    output=$(curl --silent --max-time 3 --unix-socket "$CONTROL_SOCKET" http://localhost/version 2>/dev/null) || return 1
    output=${output//[[:space:]]/}
    [[ $output =~ ^[0-9][0-9A-Za-z.+-]*$ ]] || return 1
    printf '%s' "$output"
}

# --- Service file ------------------------------------------------------------------------------------------

unit_show() {
    LC_ALL=C systemctl show "$UNIT" --no-pager \
        -p LoadState,FragmentPath,DropInPaths,ExecStart,WorkingDirectory,ReadWritePaths,ActiveState 2>/dev/null || true
}

# Splits the output of "systemctl show" (Key=Value lines) into UNIT_INFO.
parse_unit_show() {
    local line
    UNIT_INFO=()
    while IFS= read -r line || [[ -n $line ]]; do
        [[ $line == *=* ]] || continue
        UNIT_INFO[${line%%=*}]=${line#*=}
    done <<<"$1"
}

# Does the effective service file (UNIT_INFO) start the client $1 in the working folder $2?
unit_matches() {
    local client=$1 folder=$2 word found_etc=0 found_dir=0
    [[ ${UNIT_INFO[LoadState]:-} == loaded ]] || return 1
    [[ ${UNIT_INFO[ExecStart]:-} == *"{ path=$client ;"* ]] || return 1
    [[ ${UNIT_INFO[ExecStart]:-} == *"argv[]=$client --caddr $CONTROL_SOCKET service ;"* ]] || return 1
    [[ ${UNIT_INFO[WorkingDirectory]:-} == "$folder" ]] || return 1
    local IFS=' '
    for word in ${UNIT_INFO[ReadWritePaths]:-}; do
        if [[ $word == /etc ]]; then
            found_etc=1
        elif [[ $word == "$folder" ]]; then
            found_dir=1
        fi
    done
    ((found_etc && found_dir))
}

# Content of the drop-in that points the service at the client $1 with the working folder $2.
render_dropin() {
    local client=$1 folder=$2
    printf '%s\n' \
        '# SPDX-License-Identifier: GPL-3.0-only' \
        '# Copyright (C) 2026 derHouwer' \
        "$DROPIN_MARK" \
        '#' \
        '# Written by celata-setup.sh: points the Celata service at the hide.me client of this computer.' \
        '# "bash celata-setup.sh --uninstall" removes it.' \
        '[Service]' \
        "WorkingDirectory=$folder" \
        'ExecStart=' \
        "ExecStart=$client --caddr $CONTROL_SOCKET service" \
        'ReadWritePaths=' \
        "ReadWritePaths=$folder /etc -/run/systemd/resolve"
}

# Does the package file of the family need a drop-in for this client? Never with a service file made by hand.
dropin_needed() {
    local client=$1 folder=$2 manual_unit=$3 expected
    ((manual_unit == 0)) || return 1
    expected=$(expected_client "$CL_FAMILY")
    [[ $client != "${expected% *}" || $folder != "${expected#* }" ]]
}

# --- Sums and download -------------------------------------------------------------------------------------

# Prints the sum of the file named $1 from the SHA256SUMS file $2. Returns 2 if there is no such file and
# 1 if it has no line for the name (an exact match of the whole name).
sums_line_for() {
    local name=$1 file=$2 line
    [[ -f $file ]] || return 2
    while IFS= read -r line || [[ -n $line ]]; do
        if [[ $line =~ ^([0-9a-fA-F]{64})[[:space:]]+\*?(.+)$ ]] && [[ ${BASH_REMATCH[2]} == "$name" ]]; then
            printf '%s' "${BASH_REMATCH[1],,}"
            return 0
        fi
    done <"$file"
    return 1
}

# Checks the file $1 against the sum of its name in SHA256SUMS $2 and keeps the expected sum in EXPECTED_SUM.
# Returns 0 for a match, 1 if SHA256SUMS is missing, 2 if it has no line for the file, 3 if the sum is
# wrong, 4 if the file is missing.
verify_file() {
    local file=$1 sums=$2 name expected actual rc=0
    name=$(basename -- "$file")
    expected=$(sums_line_for "$name" "$sums") || rc=$?
    case $rc in
        2) return 1 ;;
        1) return 2 ;;
    esac
    [[ -f $file ]] || return 4
    actual=$(sha256sum -- "$file" | cut -d' ' -f1)
    if [[ $actual != "$expected" ]]; then
        return 3
    fi
    EXPECTED_SUM=$expected
}

# Checks this script against its own line of SHA256SUMS (the file name there is always celata-setup.sh).
verify_self() {
    local sums=$1 expected actual rc=0
    expected=$(sums_line_for celata-setup.sh "$sums") || rc=$?
    case $rc in
        2) return 1 ;;
        1) return 2 ;;
    esac
    actual=$(sha256sum -- "$SCRIPT_PATH" | cut -d' ' -f1)
    [[ $actual == "$expected" ]] || return 3
}

download_url() {
    printf '%s%s/%s' "$RELEASE_URL_BASE" "$CELATA_VERSION" "$1"
}

# Tool for the download: curl, otherwise wget; fails if neither exists.
fetch_tool() {
    if command -v curl >/dev/null 2>&1; then
        printf 'curl'
    elif command -v wget >/dev/null 2>&1; then
        printf 'wget'
    else
        return 1
    fi
}

# Downloads $1 (an address of the release page) to the file $2, over https only.
fetch() {
    local url=$1 target=$2
    case $(fetch_tool) in
        curl) curl --proto =https --proto-redir =https --tlsv1.2 -fL --silent --show-error -o "$target" -- "$url" ;;
        wget) wget --https-only --quiet -O "$target" -- "$url" ;;
    esac
}

# --- Blocked internet ---------------------------------------------------------------------------------------

# Does a table of routes (JSON of "ip -j route show") hold the default route over the loopback device?
routes_block_internet() {
    local object
    while IFS= read -r object; do
        if [[ $object =~ \"dst\"[[:space:]]*:[[:space:]]*\"default\" && $object =~ \"dev\"[[:space:]]*:[[:space:]]*\"lo\" ]]; then
            return 0
        fi
    done < <(printf '%s' "$1" | grep -o '{[^{}]*}' || true)
    return 1
}

# Is the internet blocked by the kill switch of the service? An unreadable table counts as free.
is_blocked() {
    local v4 v6
    v4=$(LC_ALL=C ip -j route show table "$BLOCK_TABLE" 2>/dev/null) || v4=''
    v6=$(LC_ALL=C ip -6 -j route show table "$BLOCK_TABLE" 2>/dev/null) || v6=''
    routes_block_internet "$v4" || routes_block_internet "$v6"
}

# --- Record of changes (what this script set up itself) ---------------------------------------------------

# Reads the record file $1 into STATE; lines of keys this version does not know are kept in STATE_EXTRA.
load_state() {
    local file=$1 line key value
    STATE=()
    STATE_EXTRA=()
    [[ -r $file ]] || return 0
    while IFS= read -r line || [[ -n $line ]]; do
        if [[ -z $line || $line == '#'* || $line != *=* ]]; then
            continue
        fi
        key=${line%%=*}
        value=${line#*=}
        case $key in
            format | package | package_version | dropin | user) STATE[$key]=$value ;;
            *) STATE_EXTRA+=("$line") ;;
        esac
    done <"$file"
}

# Content of the record file for the current STATE.
render_state() {
    local key line
    printf '%s\n' '# Written by celata-setup.sh. Lists what the script set up itself.' 'format=1'
    for key in package package_version dropin user; do
        if [[ -n ${STATE[$key]:-} ]]; then
            printf '%s=%s\n' "$key" "${STATE[$key]}"
        fi
    done
    for line in "${STATE_EXTRA[@]}"; do
        printf '%s\n' "$line"
    done
}

state_add_user() {
    local name=$1 existing IFS=' '
    for existing in ${STATE[user]:-}; do
        if [[ $existing == "$name" ]]; then
            return 0
        fi
    done
    STATE[user]="${STATE[user]:+${STATE[user]} }$name"
}

# --- Findings ---------------------------------------------------------------------------------------------

# record_check ID STATUS DETAIL NEXT; STATUS is ok, missing, problem, hint or unknown.
record_check() {
    CHECK_STATUS[$1]=$2
    CHECK_DETAIL[$1]=${3:-}
    CHECK_NEXT[$1]=${4:-}
}

mark_of_status() {
    case $1 in
        ok) mark ok ;;
        missing | problem) mark fail ;;
        hint) mark hint ;;
        *) mark unknown ;;
    esac
}

# Prints the findings in the fixed order.
render_checks() {
    local id
    for id in "${CHECK_ORDER[@]}"; do
        [[ -n ${CHECK_STATUS[$id]:-} ]] || continue
        printf '  %s %s: %s\n' "$(mark_of_status "${CHECK_STATUS[$id]}")" "$(fmt "chk_name_$id")" "${CHECK_DETAIL[$id]}"
        if [[ -n ${CHECK_NEXT[$id]:-} ]]; then
            printf '      %s\n' "${CHECK_NEXT[$id]}"
        fi
    done
}

# 1 if one finding is a failure or a step failed or was left out, otherwise 0. Hints and things that could
# not be checked do not count.
exit_code_from_checks() {
    local id
    for id in "${CHECK_ORDER[@]}"; do
        case ${CHECK_STATUS[$id]:-} in
            missing | problem) return "$RC_OPEN" ;;
        esac
    done
    if ((${#SKIPPED_STEPS[@]} > 0)); then
        return "$RC_OPEN"
    fi
    return "$RC_OK"
}

# Ending of every run: what was done, what is open, notes, and what the user still has to do.
summary() {
    local id item open=() notes=()
    for id in "${CHECK_ORDER[@]}"; do
        case ${CHECK_STATUS[$id]:-} in
            missing | problem) open+=("$(fmt "chk_name_$id"): ${CHECK_DETAIL[$id]}") ;;
            hint | unknown)
                notes+=("$(fmt "chk_name_$id"): ${CHECK_DETAIL[$id]}")
                if [[ -n ${CHECK_NEXT[$id]:-} ]]; then
                    notes+=("    ${CHECK_NEXT[$id]}")
                fi
                ;;
        esac
    done
    for item in "${SKIPPED_STEPS[@]}"; do
        open+=("$item")
    done
    printf '\n'
    if ((${#DONE_STEPS[@]} > 0)); then
        say summary_done
        for item in "${DONE_STEPS[@]}"; do
            printf '  - %s\n' "$item"
        done
    fi
    if ((${#open[@]} > 0)); then
        say summary_open
        for item in "${open[@]}"; do
            printf '  - %s\n' "$item"
        done
    elif ((${#DONE_STEPS[@]} == 0)); then
        say summary_nothing_missing
    fi
    if ((${#notes[@]} > 0)); then
        say summary_notes
        for item in "${notes[@]}"; do
            if [[ $item == '    '* ]]; then
                printf '    %s\n' "${item#    }"
            else
                printf '  - %s\n' "$item"
            fi
        done
    fi
    if ((NEED_RELOGIN)); then
        say summary_relogin
    fi
    if ((NEED_RESTART)); then
        say summary_restart "$UNIT"
    fi
}

# --- Finding the facts --------------------------------------------------------------------------------------

# The state of the rule for polkit under /etc: absent, same, differs or unknown (needs administrator rights).
etc_rule_state() {
    local sum
    if ((EUID == 0)) || [[ -x $ETC_RULES_DIR && -r $ETC_RULES_DIR ]]; then
        [[ -e $ETC_RULE ]] || {
            printf 'absent'
            return 0
        }
        sum=$(sha256sum -- "$ETC_RULE" | cut -d' ' -f1)
    elif ((USE_ROOT_FOR_RULE)); then
        as_root test -e "$ETC_RULE" || {
            printf 'absent'
            return 0
        }
        sum=$(as_root sha256sum -- "$ETC_RULE" | cut -d' ' -f1)
    else
        printf 'unknown'
        return 0
    fi
    if [[ $sum == "$CELATA_RULE_SHA256" ]]; then
        printf 'same'
    else
        printf 'differs'
    fi
}

collect_package_facts() {
    local wanted=$CELATA_VERSION-$CELATA_PKGREL
    PKG_STATE=none
    PKG_VERSION=''
    PKG_FILES_MISSING=0
    [[ -n $CL_FAMILY ]] || return 0
    if PKG_VERSION=$(installed_package_version); then
        if [[ $PKG_VERSION == "$wanted" ]]; then
            PKG_STATE=current
        elif version_at_least "$PKG_VERSION" "$wanted"; then
            PKG_STATE=newer
        else
            PKG_STATE=old
        fi
        if [[ ! -e $PACKAGE_UNIT || ! -e $PACKAGE_RULE || ! -e $PACKAGE_SYSUSERS ]]; then
            PKG_FILES_MISSING=1
        fi
    else
        PKG_VERSION=''
    fi
}

# Parts of a setup made by hand: a service file or rule under /etc, a program outside of the package.
detect_manual_parts() {
    MANUAL_UNIT_PRESENT=0
    MANUAL_RULE_PRESENT=0
    MANUAL_PROGRAM_PRESENT=0
    if [[ -e $MANUAL_UNIT ]]; then
        MANUAL_UNIT_PRESENT=1
    fi
    case $(etc_rule_state) in
        same | differs) MANUAL_RULE_PRESENT=1 ;;
        unknown) MANUAL_RULE_PRESENT=unknown ;;
    esac
    if [[ -x $MANUAL_PROGRAM ]]; then
        MANUAL_PROGRAM_PRESENT=1
    elif [[ $PKG_STATE == none ]] && command -v celata >/dev/null 2>&1; then
        MANUAL_PROGRAM_PRESENT=1
    fi
}

manual_setup_present() {
    [[ $MANUAL_UNIT_PRESENT == 1 || $MANUAL_RULE_PRESENT == 1 || $MANUAL_PROGRAM_PRESENT == 1 ]]
}

# Where the installed package came from: setup (this script), preexisting, or none.
package_origin() {
    if [[ $PKG_STATE == none ]]; then
        printf 'none'
    elif [[ ${STATE[package]:-} == setup ]]; then
        printf 'setup'
    else
        printf 'preexisting'
    fi
}

# State of the drop-in of this script: absent, current, stale (ours but another content) or foreign.
dropin_state() {
    if [[ ! -e $DROPIN_FILE ]]; then
        printf 'absent'
    elif ! grep -qxF -- "$DROPIN_MARK" "$DROPIN_FILE" 2>/dev/null; then
        printf 'foreign'
    elif [[ -n $CLIENT_PATH && $(cat -- "$DROPIN_FILE") == "$(render_dropin "$CLIENT_PATH" "$CLIENT_DIR")" ]]; then
        printf 'current'
    else
        printf 'stale'
    fi
}

group_has_member() {
    local line=$1 user=$2 members member
    members=${line##*:}
    local IFS=','
    for member in $members; do
        if [[ $member == "$user" ]]; then
            return 0
        fi
    done
    return 1
}

# --- The checks ---------------------------------------------------------------------------------------------

check_system() {
    local label
    case $CL_KIND in
        tested)
            label=$(base_label)
            if [[ $OS_ID != "$CL_BASE" ]]; then
                label=$(fmt label_as "$OS_PRETTY" "$label")
            fi
            record_check system ok "$(fmt chk_system_tested "$label")"
            ;;
        derivative)
            record_check system hint "$(fmt chk_system_derivative "$OS_PRETTY" "$(base_label)")"
            ;;
        *)
            record_check system problem "$(fmt "reason_$CL_REASON" "${OS_PRETTY:-?}")" "$(fmt next_unsupported)"
            ;;
    esac
}

check_blocked() {
    if is_blocked; then
        record_check blocked hint "$(fmt chk_blocked_yes)" "$(fmt block_exit "$UNIT")"
    else
        record_check blocked ok "$(fmt chk_blocked_no)"
    fi
}

# Looks for the client; sets CLIENT_STATE to ok, missing, ambiguous, incomplete, unsafe-path or untrusted.
evaluate_client() {
    local places=("${CLIENT_PLACES[@]}") chosen='' folder bad
    CLIENT_LIST=()
    CLIENT_PATH='' CLIENT_DIR='' CLIENT_STATE=missing CLIENT_BAD_PATH=''
    if [[ -n $OPT_CLIENT ]]; then
        places+=("$OPT_CLIENT")
    fi
    mapfile -t CLIENT_LIST < <(find_clients "${places[@]}")
    if ((${#CLIENT_LIST[@]} == 0)); then
        return 0
    fi
    if [[ -n $OPT_CLIENT ]]; then
        chosen=$(realpath -e -- "$OPT_CLIENT" 2>/dev/null) || chosen=''
        if [[ -z $chosen || ! -f $chosen || ! -x $chosen ]]; then
            return 0
        fi
    elif ((${#CLIENT_LIST[@]} > 1)); then
        CLIENT_STATE=ambiguous
        return 0
    else
        chosen=${CLIENT_LIST[0]}
    fi
    CLIENT_PATH=$chosen
    if ! folder=$(client_dir "$chosen" "${CLIENT_DIR_PLACES[@]}"); then
        CLIENT_STATE=incomplete
        return 0
    fi
    CLIENT_DIR=$folder
    if ! plain_path "$CLIENT_PATH" || ! plain_path "$CLIENT_DIR"; then
        CLIENT_STATE=unsafe-path
        return 0
    fi
    if ! bad=$(client_trusted "$CLIENT_PATH" "$CLIENT_DIR"); then
        CLIENT_STATE=untrusted
        CLIENT_BAD_PATH=$bad
        return 0
    fi
    CLIENT_STATE=ok
}

check_client() {
    case $CLIENT_STATE in
        ok) record_check client ok "$(fmt chk_client_ok "$CLIENT_PATH" "$CLIENT_DIR")" ;;
        missing) record_check client problem "$(fmt chk_client_missing)" "$(fmt next_client_missing)" ;;
        ambiguous)
            record_check client problem "$(fmt chk_client_ambiguous "$(join_by ', ' "${CLIENT_LIST[@]}")")" \
                "$(fmt next_client_ambiguous)"
            ;;
        incomplete)
            record_check client problem "$(fmt chk_client_incomplete "$CLIENT_PATH")" "$(fmt next_client_missing)"
            ;;
        unsafe-path) record_check client problem "$(fmt chk_client_unsafe_path "$CLIENT_PATH")" ;;
        untrusted)
            record_check client problem "$(fmt chk_client_untrusted "$CLIENT_BAD_PATH")" \
                "$(fmt next_client_untrusted "$CLIENT_DIR" "$CLIENT_DIR")"
            ;;
    esac
}

check_client_version() {
    local version
    if version=$(service_client_version); then
        if [[ $version == "$EXPECTED_CLIENT_VERSION" ]]; then
            record_check client_version ok "$(fmt chk_version_ok "$version")"
        else
            record_check client_version hint "$(fmt chk_version_other "$version" "$EXPECTED_CLIENT_VERSION")"
        fi
    else
        record_check client_version unknown "$(fmt chk_version_unknown)"
    fi
}

check_token() {
    if [[ -z $CLIENT_DIR ]]; then
        record_check token unknown "$(fmt chk_token_unknown)"
    elif token_present "$CLIENT_DIR"; then
        record_check token ok "$(fmt chk_token_ok)"
    else
        record_check token hint "$(fmt chk_token_missing)" "$(fmt next_token)"
    fi
}

check_package() {
    local origin
    if [[ -z $CL_FAMILY ]]; then
        record_check package unknown "$(fmt chk_package_nosystem)"
        return 0
    fi
    origin=$(package_origin)
    case $PKG_STATE in
        none)
            if manual_setup_present; then
                record_check package hint "$(fmt chk_package_manual)"
            else
                record_check package missing "$(fmt chk_package_none)"
            fi
            ;;
        old) record_check package hint "$(fmt chk_package_old "$PKG_VERSION" "$CELATA_VERSION-$CELATA_PKGREL")" ;;
        *) record_check package ok "$(fmt "chk_package_ok_$origin" "$PKG_VERSION")" ;;
    esac
}

check_unit() {
    local why=''
    DROPIN_STATE=absent
    UNIT_ACTIVE=0
    if [[ $CLIENT_STATE != ok ]]; then
        record_check unit unknown "$(fmt chk_unit_noclient)"
        return 0
    fi
    parse_unit_show "$(unit_show)"
    if [[ ${UNIT_INFO[ActiveState]:-} == active ]]; then
        UNIT_ACTIVE=1
    fi
    DROPIN_STATE=$(dropin_state)
    if unit_matches "$CLIENT_PATH" "$CLIENT_DIR"; then
        record_check unit ok "$(fmt chk_unit_ok)"
        return 0
    fi
    if [[ ${UNIT_INFO[LoadState]:-} != loaded ]]; then
        record_check unit missing "$(fmt chk_unit_absent)"
        return 0
    fi
    if [[ ${UNIT_INFO[WorkingDirectory]:-} != "$CLIENT_DIR" ]]; then
        why=WorkingDirectory
    elif [[ ${UNIT_INFO[ExecStart]:-} != *"{ path=$CLIENT_PATH ;"* ]]; then
        why=ExecStart
    else
        why=ReadWritePaths
    fi
    if ((MANUAL_UNIT_PRESENT)) || ! dropin_needed "$CLIENT_PATH" "$CLIENT_DIR" "$MANUAL_UNIT_PRESENT"; then
        record_check unit problem "$(fmt chk_unit_differs "$why")" "$(fmt next_unit_differs "${UNIT_INFO[DropInPaths]:-}")"
    else
        record_check unit missing "$(fmt chk_unit_needs_dropin "$why")"
    fi
}

check_group() {
    local line
    GROUP_PRESENT=0
    if line=$(LC_ALL=C getent group "$GROUP" 2>/dev/null); then
        GROUP_PRESENT=1
        GROUP_LINE=$line
        record_check group ok "$(fmt chk_group_ok "$GROUP")"
    else
        GROUP_LINE=''
        record_check group missing "$(fmt chk_group_missing "$GROUP")"
    fi
}

check_member() {
    MEMBER_PRESENT=0
    if ((GROUP_PRESENT)) && group_has_member "$GROUP_LINE" "$USER_NAME"; then
        MEMBER_PRESENT=1
        record_check member ok "$(fmt chk_member_ok "$USER_NAME")"
    else
        record_check member missing "$(fmt chk_member_missing "$USER_NAME" "$GROUP")"
    fi
}

check_session() {
    local groups
    if ((EUID == 0)); then
        record_check session unknown "$(fmt chk_session_root)"
    elif ((MEMBER_PRESENT == 0)); then
        record_check session unknown "$(fmt chk_session_nomember)"
    elif groups=$(LC_ALL=C id -nG 2>/dev/null) && [[ " ${groups//$'\n'/ } " == *" $GROUP "* ]]; then
        record_check session ok "$(fmt chk_session_ok)"
    else
        NEED_RELOGIN=1
        record_check session hint "$(fmt chk_session_relogin)"
    fi
}

check_polkit() {
    local version rule
    if ! version=$(polkit_ok); then
        record_check polkit problem "$(fmt chk_polkit_old "$version" "$MIN_POLKIT")"
        return 0
    fi
    if [[ -e $PACKAGE_RULE ]]; then
        record_check polkit ok "$(fmt chk_polkit_package)"
        return 0
    fi
    rule=$(etc_rule_state)
    case $rule in
        same) record_check polkit ok "$(fmt chk_polkit_manual)" ;;
        differs) record_check polkit problem "$(fmt chk_polkit_differs)" "$(fmt next_polkit_differs)" ;;
        absent)
            if [[ $PKG_STATE == none ]]; then
                record_check polkit missing "$(fmt chk_polkit_absent)"
            else
                record_check polkit missing "$(fmt chk_polkit_lost)"
            fi
            ;;
        *) record_check polkit unknown "$(fmt chk_polkit_unknown)" "$(fmt next_polkit_unknown "$SCRIPT_PATH")" ;;
    esac
}

# Is systemd-resolved running? Judged from the output of "systemctl is-active".
dns_state() {
    if [[ $1 == active ]]; then
        printf 'resolved'
    else
        printf 'none'
    fi
}

check_dns() {
    local output
    output=$(LC_ALL=C systemctl is-active systemd-resolved.service 2>/dev/null) || true
    if [[ $(dns_state "$output") == resolved ]]; then
        record_check dns hint "$(fmt chk_dns_leak)" "$(fmt next_dns)"
    else
        record_check dns ok "$(fmt chk_dns_none)"
    fi
}

# Is the name of the tray watcher owned? From the return value and output of "busctl --user status NAME".
tray_state() {
    local rc=$1 output=$2
    if ((rc == 0)); then
        printf 'present'
    elif [[ ${output,,} == *'no such name'* || ${output,,} == *'not found'* || ${output,,} == *'could not get pid'* ]]; then
        printf 'absent'
    else
        printf 'unknown'
    fi
}

check_tray() {
    local rc=0 output
    output=$(LC_ALL=C busctl --user status org.kde.StatusNotifierWatcher 2>&1) || rc=$?
    case $(tray_state "$rc" "$output") in
        present) record_check tray ok "$(fmt chk_tray_ok)" ;;
        absent) record_check tray hint "$(fmt chk_tray_missing)" ;;
        *) record_check tray unknown "$(fmt chk_tray_unknown)" ;;
    esac
}

# The first active unit "hide.me@…" from the output of "systemctl list-units"; empty if there is none.
other_service_state() {
    local line
    while IFS= read -r line; do
        line=${line#"${line%%[![:space:]]*}"}
        if [[ $line == hide.me@* ]]; then
            printf '%s' "${line%%[[:space:]]*}"
            return 0
        fi
    done <<<"$1"
    return 0
}

check_other_service() {
    local output unit
    output=$(LC_ALL=C systemctl list-units 'hide.me@*' --state=active --no-legend --plain --no-pager 2>/dev/null) || output=''
    unit=$(other_service_state "$output")
    if [[ -n $unit ]]; then
        record_check other_service hint "$(fmt chk_other_service "$unit")" "$(fmt next_other_service "$unit")"
    else
        record_check other_service ok "$(fmt chk_other_none)"
    fi
}

collect_checks() {
    CHECK_STATUS=() CHECK_DETAIL=() CHECK_NEXT=()
    check_system
    check_blocked
    evaluate_client
    check_client
    check_client_version
    check_token
    load_state "$STATE_FILE"
    collect_package_facts
    detect_manual_parts
    check_package
    check_unit
    check_group
    check_member
    check_session
    check_polkit
    check_dns
    check_tray
    check_other_service
}

# --- The plan -----------------------------------------------------------------------------------------------

step_key() {
    printf 'step_%s' "${1//-/_}"
}

# Does the step need administrator rights? Everything but the checks does.
step_needs_root() {
    printf 'yes'
}

# The command a step runs, as the plan shows it.
step_command() {
    local file=''
    case $1 in
        package-install | package-update | package-reinstall)
            file=$(package_file)
            pm_build "${1#package-}" "/var/tmp/celata-setup.XXXXXXXX/$file"
            printf '%s' "$(printf '%q ' "${PM_ARGV[@]}")" | sed 's/ $//'
            ;;
        package-remove)
            pm_build remove ''
            printf '%s' "$(printf '%q ' "${PM_ARGV[@]}")" | sed 's/ $//'
            ;;
        dropin-write) printf 'install -m 0644 %s' "$DROPIN_FILE" ;;
        dropin-remove | dropin-delete) printf 'rm %s' "$DROPIN_FILE" ;;
        reload) printf 'systemctl daemon-reload' ;;
        group-create) printf 'groupadd --system %s' "$GROUP" ;;
        group-add) printf 'usermod -aG %s %s' "$GROUP" "$USER_NAME" ;;
        service-stop) printf 'systemctl stop %s' "$UNIT" ;;
        state-write) printf 'install -m 0644 %s' "$STATE_FILE" ;;
        state-delete) printf 'rm %s' "$STATE_FILE" ;;
    esac
}

# add_step ID CONSENT: consent is plan, account, overwrite or separate.
add_step() {
    PLAN_ID+=("$1")
    PLAN_ROOT+=("$(step_needs_root "$1")")
    PLAN_CONSENT+=("$2")
    PLAN_CMD+=("$(step_command "$1")")
}

plan_has() {
    local id
    for id in "${PLAN_ID[@]}"; do
        if [[ $id == "$1" ]]; then
            return 0
        fi
    done
    return 1
}

# The plan of a set-up run, from the findings and facts. Order: package, drop-in, reload, group, record.
build_plan() {
    local origin consent package_step=''
    PLAN_ID=() PLAN_ROOT=() PLAN_CONSENT=() PLAN_CMD=()
    origin=$(package_origin)
    consent=plan
    if [[ $origin != setup ]]; then
        consent=overwrite
    fi
    case $PKG_STATE in
        none)
            if manual_setup_present; then
                add_step package-install separate
                package_step=separate
            else
                add_step package-install plan
                package_step=plan
            fi
            ;;
        old)
            add_step package-update "$consent"
            package_step=$consent
            ;;
        *)
            if ((PKG_FILES_MISSING)); then
                add_step package-reinstall "$consent"
                package_step=$consent
            fi
            ;;
    esac
    if [[ $CLIENT_STATE == ok ]]; then
        if dropin_needed "$CLIENT_PATH" "$CLIENT_DIR" "$MANUAL_UNIT_PRESENT"; then
            case $DROPIN_STATE in
                absent | stale) add_step dropin-write plan ;;
                foreign) add_step dropin-write overwrite ;;
            esac
        elif [[ $DROPIN_STATE == current || $DROPIN_STATE == stale ]]; then
            add_step dropin-remove overwrite
        fi
    fi
    if plan_has dropin-write || plan_has dropin-remove; then
        add_step reload plan
    fi
    if ((GROUP_PRESENT == 0)) && [[ -z $package_step || $package_step == separate ]]; then
        add_step group-create plan
    fi
    if ((MEMBER_PRESENT == 0)); then
        add_step group-add account
    fi
    # The record is written with the first step that needs no question of its own; steps that ask on their own
    # get it when they are agreed to (see ask_separate_steps).
    if plan_has_plain_steps; then
        add_step state-write plan
    fi
}

# The plan of a removal, from what the script set up (the record) and what is installed.
build_uninstall_plan() {
    local origin
    PLAN_ID=() PLAN_ROOT=() PLAN_CONSENT=() PLAN_CMD=()
    origin=$(package_origin)
    if [[ -e $DROPIN_FILE ]] && grep -qxF -- "$DROPIN_MARK" "$DROPIN_FILE" 2>/dev/null; then
        add_step dropin-delete plan
        add_step reload plan
    fi
    if [[ $origin == setup ]]; then
        add_step package-remove plan
    elif [[ $origin != none ]]; then
        add_step package-remove separate
    fi
    if ((${#PLAN_ID[@]} > 0)) && ((UNIT_ACTIVE)) && plan_has package-remove; then
        PLAN_ID=(service-stop "${PLAN_ID[@]}")
        PLAN_ROOT=(yes "${PLAN_ROOT[@]}")
        PLAN_CONSENT=(plan "${PLAN_CONSENT[@]}")
        PLAN_CMD=("$(step_command service-stop)" "${PLAN_CMD[@]}")
    fi
    if [[ -e $STATE_FILE ]]; then
        add_step state-delete plan
    fi
}

# Removes the step at position $1 from the plan.
drop_step() {
    local at=$1
    PLAN_ID=("${PLAN_ID[@]:0:at}" "${PLAN_ID[@]:at+1}")
    PLAN_ROOT=("${PLAN_ROOT[@]:0:at}" "${PLAN_ROOT[@]:at+1}")
    PLAN_CONSENT=("${PLAN_CONSENT[@]:0:at}" "${PLAN_CONSENT[@]:at+1}")
    PLAN_CMD=("${PLAN_CMD[@]:0:at}" "${PLAN_CMD[@]:at+1}")
}

# Activity of a step without the command: "Install the Celata package".
step_activity() {
    local text
    text=$(text_of "$(step_key "$1")")
    text=${text% (%s)}
    # shellcheck disable=SC2059 # the message text is the format on purpose
    printf -- "$text"
}

show_plan() {
    local i
    say plan_intro
    for ((i = 0; i < ${#PLAN_ID[@]}; i++)); do
        printf '  %d. ' "$((i + 1))"
        fmt "$(step_key "${PLAN_ID[i]}")" "${PLAN_CMD[i]}"
        case ${PLAN_CONSENT[i]} in
            overwrite | separate) printf ' %s' "$(fmt plan_asks_separately)" ;;
        esac
        printf '\n'
    done
    if ((EUID != 0)); then
        say plan_sudo_note
    fi
}

# Question for a step that asks on its own.
step_question_key() {
    case $1 in
        package-install) printf 'ask_install_beside_manual' ;;
        package-update) printf 'ask_update_preexisting' ;;
        package-reinstall) printf 'ask_reinstall_preexisting' ;;
        package-remove) printf 'ask_remove_preexisting' ;;
        dropin-write) printf 'ask_dropin_replace' ;;
        dropin-remove) printf 'ask_dropin_remove' ;;
        *) printf 'ask_step_generic' ;;
    esac
}

# Is $1 in the list that follows?
in_list() {
    local needle=$1 item
    shift
    for item in "$@"; do
        if [[ $item == "$needle" ]]; then
            return 0
        fi
    done
    return 1
}

# Asks the steps with their own question (overwrite, separate) and removes the ones answered with no. A step is
# asked once per run: the answer is kept, also when the plan is built again.
ask_separate_steps() {
    local i=0 id
    while ((i < ${#PLAN_ID[@]})); do
        id=${PLAN_ID[i]}
        case ${PLAN_CONSENT[i]} in
            overwrite | separate)
                if in_list "$id" "${ASKED_YES[@]}"; then
                    i=$((i + 1))
                elif in_list "$id" "${ASKED_NO[@]}"; then
                    drop_step "$i"
                elif ask "$(step_question_key "$id")" no "${PLAN_CONSENT[i]}" "$(step_activity "$id")"; then
                    ASKED_YES+=("$id")
                    i=$((i + 1))
                else
                    ASKED_NO+=("$id")
                    SKIPPED_STEPS+=("$(fmt step_declined "$(step_activity "$id")")")
                    drop_step "$i"
                fi
                ;;
            *) i=$((i + 1)) ;;
        esac
    done
    if ((OPT_UNINSTALL == 0)) && ((${#PLAN_ID[@]} > 0)) && ! plan_has state-write; then
        add_step state-write plan
    fi
    prune_plan
}

# Takes out steps that only make sense together with another one that is gone now.
prune_plan() {
    local i
    if plan_has service-stop && ! plan_has package-remove; then
        for ((i = 0; i < ${#PLAN_ID[@]}; i++)); do
            if [[ ${PLAN_ID[i]} == service-stop ]]; then
                drop_step "$i"
                break
            fi
        done
    fi
    if plan_has reload && ! plan_has dropin-write && ! plan_has dropin-remove && ! plan_has dropin-delete; then
        for ((i = 0; i < ${#PLAN_ID[@]}; i++)); do
            if [[ ${PLAN_ID[i]} == reload ]]; then
                drop_step "$i"
                break
            fi
        done
    fi
    if plan_has state-write && ((${#PLAN_ID[@]} == 1)); then
        drop_step 0
    fi
}

# Does the plan hold a step that needs no question of its own? Such a plan asks once for the whole run.
plan_has_plain_steps() {
    local consent
    for consent in "${PLAN_CONSENT[@]}"; do
        if [[ $consent == plan || $consent == account ]]; then
            return 0
        fi
    done
    return 1
}

# --- The steps ----------------------------------------------------------------------------------------------

STEP_REASON=''

# Copies the checked package to a folder only root can change, checks the copy again and hands exactly that
# path to the package manager.
step_package() {
    local action=$1 file target sum
    file=$(package_file)
    if [[ -z $EXPECTED_SUM || ! -f $PKG_PATH ]]; then
        STEP_REASON=$(fmt err_package_unavailable)
        return 1
    fi
    make_root_pkgdir || return 1
    target=$PKGDIR/$file
    as_root install -m 0644 -- "$PKG_PATH" "$target" || return 1
    sum=$(as_root sha256sum -- "$target" | cut -d' ' -f1) || return 1
    if [[ $sum != "$EXPECTED_SUM" ]]; then
        remove_root_pkgdir
        STEP_REASON=$(fmt err_sum_changed)
        return 1
    fi
    pm_build "$action" "$target" || return 1
    if ! as_root "${PM_ARGV[@]}"; then
        remove_root_pkgdir
        return 1
    fi
    remove_root_pkgdir
    if [[ $action == install ]]; then
        STATE[package]=setup
    elif [[ -z ${STATE[package]:-} ]]; then
        STATE[package]=preexisting
    fi
    STATE[package_version]=$CELATA_VERSION-$CELATA_PKGREL
}

step_package_remove() {
    pm_build remove '' || return 1
    as_root "${PM_ARGV[@]}" || return 1
    unset 'STATE[package]' 'STATE[package_version]'
}

step_dropin_write() {
    local temporary=$WORKDIR/dropin.conf
    render_dropin "$CLIENT_PATH" "$CLIENT_DIR" >"$temporary"
    as_root install -d -m 0755 -- "$DROPIN_DIR" || return 1
    root_put_atomic "$temporary" "$DROPIN_FILE" 0644 || return 1
    STATE[dropin]=$DROPIN_FILE
}

step_dropin_delete() {
    if [[ -e $DROPIN_FILE ]] && ! grep -qxF -- "$DROPIN_MARK" "$DROPIN_FILE" 2>/dev/null; then
        STEP_REASON=$(fmt err_dropin_foreign)
        return 1
    fi
    as_root rm -f -- "$DROPIN_FILE" || return 1
    as_root rmdir --ignore-fail-on-non-empty -- "$DROPIN_DIR" 2>/dev/null || true
    unset 'STATE[dropin]'
}

step_reload() {
    as_root systemctl daemon-reload || return 1
    if ((UNIT_ACTIVE)); then
        NEED_RESTART=1
    fi
}

step_group_create() {
    if getent group "$GROUP" >/dev/null 2>&1; then
        return 0
    fi
    as_root groupadd --system "$GROUP"
}

step_group_add() {
    if ! getent group "$GROUP" >/dev/null 2>&1; then
        STEP_REASON=$(fmt err_group_missing "$GROUP")
        return 1
    fi
    if ! user_acceptable "$USER_NAME"; then
        STEP_REASON=$(fmt err_user_invalid "$USER_NAME")
        return 1
    fi
    as_root usermod -aG "$GROUP" -- "$USER_NAME" || return 1
    state_add_user "$USER_NAME"
    NEED_RELOGIN=1
}

step_state_write() {
    local temporary=$WORKDIR/state
    if [[ -z ${STATE[package]:-} && $PKG_STATE != none ]]; then
        STATE[package]=preexisting
    fi
    STATE[format]=1
    render_state >"$temporary"
    as_root install -d -m 0755 -- "$STATE_DIR" || return 1
    root_put_atomic "$temporary" "$STATE_FILE" 0644
}

step_service_stop() {
    as_root systemctl stop "$UNIT"
}

step_state_delete() {
    as_root rm -f -- "$STATE_FILE" || return 1
    as_root rmdir --ignore-fail-on-non-empty -- "$STATE_DIR" 2>/dev/null || true
}

run_step() {
    STEP_REASON=''
    case $1 in
        package-install | package-update | package-reinstall) step_package "${1#package-}" ;;
        package-remove) step_package_remove ;;
        dropin-write) step_dropin_write ;;
        dropin-remove | dropin-delete) step_dropin_delete ;;
        reload) step_reload ;;
        group-create) step_group_create ;;
        group-add) step_group_add ;;
        state-write) step_state_write ;;
        service-stop) step_service_stop ;;
        state-delete) step_state_delete ;;
        *) return 1 ;;
    esac
}

# Runs the plan step by step. A failed step ends the run; what is done stays, and the record is still written.
run_plan() {
    local i id activity failed=0
    CHANGES_MADE=0
    for ((i = 0; i < ${#PLAN_ID[@]}; i++)); do
        id=${PLAN_ID[i]}
        activity=$(step_activity "$id")
        if [[ $id == state-write ]] && ((CHANGES_MADE == 0)); then
            continue
        fi
        if ((failed)) && [[ $id != state-write ]]; then
            SKIPPED_STEPS+=("$(fmt step_not_run "$activity")")
            continue
        fi
        if run_step "$id"; then
            DONE_STEPS+=("$activity")
            say step_done "$activity"
            if [[ $id != state-write && $id != state-delete ]]; then
                CHANGES_MADE=1
            fi
        else
            failed=1
            say_err step_failed "$activity" "${STEP_REASON:-$(fmt step_reason_default)}"
            SKIPPED_STEPS+=("$(fmt step_failed_item "$activity")")
        fi
    done
    return "$failed"
}

# --- Preflight: everything that can stop the run, before the first change -----------------------------------

# Ends the run with the finding of one check: its detail and next step, return value 3.
fail_check() {
    local id=$1
    {
        printf '%s: %s\n' "$(fmt "chk_name_$id")" "${CHECK_DETAIL[$id]:-}"
        if [[ -n ${CHECK_NEXT[$id]:-} ]]; then
            printf '%s\n' "${CHECK_NEXT[$id]}"
        fi
    } >&2
    exit "$RC_ABORT_CLEAN"
}

choose_client() {
    local i answer
    say client_choose_intro
    for ((i = 0; i < ${#CLIENT_LIST[@]}; i++)); do
        printf '  %d) %s\n' "$((i + 1))" "${CLIENT_LIST[i]}"
    done
    fmt ask_client_number "${#CLIENT_LIST[@]}"
    printf ' '
    if ! IFS= read -r answer; then
        printf '\n'
        die "$RC_ABORT_CLEAN" err_no_terminal
    fi
    if [[ $answer =~ ^[0-9]+$ ]] && ((answer >= 1 && answer <= ${#CLIENT_LIST[@]})); then
        OPT_CLIENT=${CLIENT_LIST[answer - 1]}
    else
        die "$RC_ABORT_CLEAN" err_client_choice
    fi
}

# Makes sure there is one usable client at a safe place; otherwise ends the run before any change.
client_gate() {
    local interactive=$1
    evaluate_client
    if [[ $CLIENT_STATE == ambiguous ]]; then
        if ((OPT_YES)) || ((interactive == 0)); then
            die "$RC_ABORT_CLEAN" err_client_ambiguous "$(join_by ', ' "${CLIENT_LIST[@]}")"
        fi
        choose_client
        evaluate_client
    fi
    if [[ $CLIENT_STATE != ok ]]; then
        check_client
        fail_check client
    fi
}

# Looks for the package file next to the script, otherwise downloads it (never in the dry run), and checks
# it against SHA256SUMS. Sets PKG_PATH, PKG_FROM and EXPECTED_SUM.
locate_package() {
    local download=$1 file sums rc=0 target
    file=$(package_file)
    sums=$SCRIPT_DIR/SHA256SUMS
    PKG_PATH='' PKG_FROM='' EXPECTED_SUM=''
    if [[ -f $sums ]]; then
        verify_self "$sums" || rc=$?
        case $rc in
            2) die "$RC_ABORT_CLEAN" err_self_no_line ;;
            3) die "$RC_ABORT_CLEAN" err_self_sum ;;
        esac
    fi
    if [[ -f $SCRIPT_DIR/$file ]]; then
        PKG_PATH=$SCRIPT_DIR/$file
        PKG_FROM=local
    else
        PKG_FROM=download
        if ((download == 0)); then
            return 0
        fi
        if [[ ! -f $sums ]]; then
            die "$RC_ABORT_CLEAN" err_sums_missing "$file"
        fi
        if ! fetch_tool >/dev/null; then
            die "$RC_ABORT_CLEAN" err_no_download_tool "$file"
        fi
        target=$WORKDIR/$file
        say preflight_downloading "$file"
        if ! fetch "$(download_url "$file")" "$target"; then
            rm -f -- "$target"
            die "$RC_ABORT_CLEAN" err_download_failed "$file"
        fi
        PKG_PATH=$target
    fi
    rc=0
    verify_file "$PKG_PATH" "$sums" || rc=$?
    case $rc in
        0) ;;
        1) die "$RC_ABORT_CLEAN" err_sums_missing "$file" ;;
        2) die "$RC_ABORT_CLEAN" err_sums_no_line "$file" ;;
        *)
            if [[ $PKG_FROM == download ]]; then
                rm -f -- "$PKG_PATH"
            fi
            die "$RC_ABORT_CLEAN" err_sums_wrong "$file"
            ;;
    esac
}

# Tries the package with the package manager without installing anything (return value 3 when it fails).
probe_package() {
    local rc=0 output
    if [[ $CL_FAMILY == deb ]] && ! apt_lists_fresh; then
        pm_build probe-update ''
        if ! as_root "${PM_ARGV[@]}"; then
            die "$RC_ABORT_CLEAN" err_probe_update
        fi
    fi
    pm_build probe "$PKG_PATH"
    output=$(LC_ALL=C "${PM_ARGV[@]}" 2>&1) || rc=$?
    if ! probe_succeeded "$rc" "$output"; then
        say_err err_probe_failed "$(printf '%s\n' "$output" | tail -n 6)"
        if [[ $CL_FAMILY == arch ]]; then
            say_err next_probe_arch
        fi
        exit "$RC_ABORT_CLEAN"
    fi
}

preflight_parts() {
    local parts=() rule
    if plan_has package-install || plan_has package-update || plan_has package-reinstall; then
        if [[ $CL_FAMILY == deb ]] && ! apt_lists_fresh; then
            parts+=("$(fmt part_lists)")
        fi
        if [[ ! -f $SCRIPT_DIR/$(package_file) ]]; then
            parts+=("$(fmt part_download)")
        fi
        parts+=("$(fmt part_probe)")
    fi
    rule=$(etc_rule_state)
    if [[ $rule == unknown ]] && ((EUID != 0)); then
        parts+=("$(fmt part_rule "$ETC_RULE")")
    fi
    join_by ', ' "${parts[@]}"
}

# The checks of a run that need neither administrator rights nor the network: blocked internet and the client.
# They come first, before any question about the plan, so that a run that cannot work stops right away, before a
# password is asked for or the network is touched. Ends the run with return value 3 at the first problem.
preflight_early() {
    if is_blocked; then
        die "$RC_ABORT_CLEAN" err_blocked "$(fmt block_exit "$UNIT")"
    fi
    client_gate 1
}

# The rest of the checks of a run that changes something: rights, package file, polkit, probe of the package manager.
preflight() {
    local parts needs_package=0 version
    parts=$(preflight_parts)
    if plan_has package-install || plan_has package-update || plan_has package-reinstall; then
        needs_package=1
    fi
    if [[ -n $parts ]]; then
        say preflight_announce "$parts"
        if ((EUID != 0)); then
            say preflight_sudo_note
        fi
    fi
    if ! sudo_available; then
        die "$RC_ABORT_CLEAN" err_no_sudo "$SCRIPT_PATH" "$USER_NAME"
    fi
    if [[ $(etc_rule_state) == unknown ]] && ((EUID != 0)); then
        as_root true || die "$RC_ABORT_CLEAN" err_no_sudo "$SCRIPT_PATH" "$USER_NAME"
        USE_ROOT_FOR_RULE=1
    fi
    if ((needs_package)); then
        locate_package 1
    fi
    if ! version=$(polkit_ok); then
        die "$RC_ABORT_CLEAN" err_polkit_old "$version" "$MIN_POLKIT"
    fi
    if ((needs_package)); then
        probe_package
    fi
}

# The checks of the dry run: no network, no administrator rights, the same stops as a real run where possible.
preflight_dry() {
    local file
    if is_blocked; then
        die "$RC_ABORT_CLEAN" err_blocked "$(fmt block_exit "$UNIT")"
    fi
    client_gate 0
    if plan_has package-install || plan_has package-update || plan_has package-reinstall; then
        file=$(package_file)
        locate_package 0
        if [[ $PKG_FROM == download ]]; then
            say dry_would_download "$(download_url "$file")"
        else
            say dry_package_checked "$file"
        fi
        if [[ $CL_FAMILY == deb ]] && ! apt_lists_fresh; then
            say dry_would_run "$(printf '%q ' apt-get update | sed 's/ $//')"
        fi
        pm_build probe "/var/tmp/celata-setup.XXXXXXXX/$file"
        say dry_would_run "$(printf '%q ' "${PM_ARGV[@]}" | sed 's/ $//')"
    fi
}

# --- Runs ----------------------------------------------------------------------------------------------------

print_header() {
    say header "$CELATA_VERSION"
    say header_system "${OS_PRETTY:-?}"
    printf '\n'
}

load_system() {
    local file
    gather_system_facts
    if file=$(os_release_file); then
        read_os_release "$file"
    fi
    classify
}

# Ends the run with return value 3 when the system is not supported.
require_supported_system() {
    if [[ $CL_KIND == unsupported ]]; then
        check_system
        fail_check system
    fi
}

derivative_question() {
    if [[ $CL_KIND != derivative ]]; then
        return 0
    fi
    if ((OPT_DRY)); then
        say dry_derivative_note "$OS_PRETTY" "$(base_label)"
        return 0
    fi
    say derivative_note "$OS_PRETTY" "$(base_label)"
    if ! ask ask_derivative no plan; then
        exit "$RC_USER_ABORT"
    fi
}

run_check_mode() {
    collect_checks
    print_header
    render_checks
    summary
    exit_code_from_checks
}

run_setup() {
    local rc=0
    collect_checks
    build_plan
    if ((${#PLAN_ID[@]} == 0)); then
        print_header
        say all_set
        render_checks
        summary
        exit_code_from_checks
        return
    fi
    if ((OPT_DRY)); then
        print_header
        derivative_question
        preflight_dry
        build_plan
        show_plan
        return 0
    fi
    print_header
    derivative_question
    preflight_early
    if ! plan_has_plain_steps; then
        ask_separate_steps
        if ((${#PLAN_ID[@]} == 0)); then
            collect_checks
            summary
            exit_code_from_checks
            return
        fi
    fi
    preflight
    if ((USE_ROOT_FOR_RULE)); then
        collect_checks
        build_plan
    fi
    show_plan
    if ! ask ask_execute yes plan; then
        exit "$RC_USER_ABORT"
    fi
    ask_separate_steps
    if ((${#PLAN_ID[@]} == 0)); then
        collect_checks
        summary
        exit_code_from_checks
        return
    fi
    run_plan || rc=1
    collect_checks
    summary
    if ((rc)); then
        return "$RC_OPEN"
    fi
    exit_code_from_checks
}

# Lists what a removal leaves behind, with the commands to remove it.
show_remains() {
    local origin
    say remains_intro
    if getent group "$GROUP" >/dev/null 2>&1; then
        say remains_group "$GROUP" "sudo groupdel $GROUP"
    fi
    say remains_client
    say remains_user_files
    if ((MANUAL_UNIT_PRESENT)); then
        say remains_manual_unit "$MANUAL_UNIT"
    fi
    if [[ $MANUAL_RULE_PRESENT == 1 ]]; then
        say remains_manual_rule "$ETC_RULE"
    fi
    if ((MANUAL_PROGRAM_PRESENT)); then
        say remains_manual_program
    fi
    if ((MANUAL_UNIT_PRESENT)) || [[ $MANUAL_RULE_PRESENT == 1 ]] || ((MANUAL_PROGRAM_PRESENT)); then
        say remains_manual_hint
    fi
    origin=$(package_origin)
    if [[ $origin != none ]] && ! plan_has package-remove; then
        pm_build remove ''
        say remains_package "$(printf '%q ' sudo "${PM_ARGV[@]}" | sed 's/ $//')"
    fi
    case $CL_FAMILY in
        deb) say remains_dependencies 'sudo apt-get autoremove' ;;
        arch) say remains_dependencies "sudo pacman -Rns \$(pacman -Qdtq)" ;;
    esac
}

run_uninstall() {
    local rc=0 output
    load_state "$STATE_FILE"
    collect_package_facts
    detect_manual_parts
    UNIT_ACTIVE=0
    output=$(LC_ALL=C systemctl is-active "$UNIT" 2>/dev/null) || true
    if [[ $output == active ]]; then
        UNIT_ACTIVE=1
    fi
    build_uninstall_plan
    print_header
    if ((${#PLAN_ID[@]} == 0)); then
        say uninstall_nothing
        show_remains
        return 0
    fi
    if plan_has service-stop; then
        say uninstall_stops_service
    fi
    show_plan
    if ((OPT_DRY)); then
        show_remains
        return 0
    fi
    if ! sudo_available; then
        die "$RC_ABORT_CLEAN" err_no_sudo "$SCRIPT_PATH" "${USER_NAME:-NAME}"
    fi
    if ! ask ask_execute yes plan; then
        exit "$RC_USER_ABORT"
    fi
    ask_separate_steps
    if ((${#PLAN_ID[@]} > 0)); then
        run_plan || rc=1
    fi
    show_remains
    summary_removal
    if ((${#SKIPPED_STEPS[@]} > 0)); then
        rc=1
    fi
    return "$rc"
}

summary_removal() {
    local item
    if ((${#DONE_STEPS[@]} > 0)); then
        printf '\n'
        say summary_done
        for item in "${DONE_STEPS[@]}"; do
            printf '  - %s\n' "$item"
        done
    fi
    if ((${#SKIPPED_STEPS[@]} > 0)); then
        say summary_open
        for item in "${SKIPPED_STEPS[@]}"; do
            printf '  - %s\n' "$item"
        done
    fi
}

main() {
    local rc=0
    detect_language
    use_utf8
    if ((BASH_VERSINFO[0] < 5)); then
        die "$RC_ABORT_CLEAN" err_bash_old "$BASH_VERSION"
    fi
    parse_args "$@" || exit "$?"
    if ((OPT_HELP)); then
        say help_text
        exit "$RC_OK"
    fi
    if ((OPT_VERSION)); then
        say version_line "$CELATA_VERSION" "$CELATA_PKGREL"
        exit "$RC_OK"
    fi
    trap on_exit EXIT
    trap on_signal INT TERM
    trap 'say_err err_internal "$LINENO"' ERR
    make_workdir
    if ((OPT_UNINSTALL == 0)); then
        resolve_user
    fi
    load_system
    if ((OPT_UNINSTALL)); then
        require_supported_system
        run_uninstall || rc=$?
    elif ((OPT_CHECK)); then
        run_check_mode || rc=$?
    else
        require_supported_system
        run_setup || rc=$?
    fi
    exit "$rc"
}

if [[ ${BASH_SOURCE[0]} == "$0" ]]; then
    main "$@"
fi
