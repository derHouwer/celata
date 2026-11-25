// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! Desktop notifications about the connection while the window of Celata is not in front.
//!
//! Which change is worth a notification and what it says is decided here without any access (`event`,
//! `should_show`, `body`). Only `spawn` talks to the notification service of the desktop, over the session
//! bus of the user. Texts name country and place, never a host name or an address: every program of the
//! session can read notifications.

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use zbus::blocking::Connection;
use zbus::zvariant::Value;

use crate::i18n::{self, Key, Language, Phrase};
use crate::look;
use crate::servers::Country;
use crate::state::DisplayState;
use crate::streaming;
use crate::text;

/// Name of the program and title of every notification.
pub const APP_NAME: &str = "Celata";
/// Icon of the notification: the logo, installed with the desktop entry.
const APP_ICON: &str = "celata";
/// Longest text of a notification.
const MAX_BODY: usize = 200;

/// A change worth a notification. Hosts are kept until the text is built and never shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Connected(Option<String>),
    Disconnected,
    Lost {
        blocked: bool,
    },
    Failed(Option<String>),
    Switched(Option<String>),
    SwitchedBack(Option<String>),
    Blocked,
    Unblocked,
    /// The message of the window after a choice in the menu (a failure, or a notice such as "Already connected
    /// to the best location").
    ActionFailed(Phrase),
}

/// What is known about a display besides itself: the target of the running connection attempt.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Shown {
    pub display: Option<DisplayState>,
    /// `StatusView::connect_target` at that time.
    pub connect_target: Option<String>,
}

/// The notification for the change from `prev` to `next`, if any. A switch reports only its result, reconnecting
/// after a change of settings nothing, and the first display after the start nothing.
pub fn event(prev: &Shown, next: &Shown) -> Option<Event> {
    let (Some(before), Some(after)) = (prev.display.as_ref(), next.display.as_ref()) else {
        return None;
    };
    let server = match after {
        DisplayState::Connected { server } => server.clone(),
        _ => None,
    };
    match (before, after) {
        (DisplayState::Connecting | DisplayState::ConnectingBlocked, DisplayState::Connected { .. }) => {
            Some(Event::Connected(server.or_else(|| prev.connect_target.clone())))
        }
        (DisplayState::Disconnecting, DisplayState::NotConnected) => Some(Event::Disconnected),
        (DisplayState::Connected { .. }, DisplayState::NotConnected) => Some(Event::Lost { blocked: false }),
        (DisplayState::Connected { .. } | DisplayState::Switching { .. }, DisplayState::Blocked) => {
            Some(Event::Lost { blocked: true })
        }
        (
            DisplayState::Connecting | DisplayState::ConnectingBlocked,
            DisplayState::NotConnected | DisplayState::Blocked,
        ) => Some(Event::Failed(prev.connect_target.clone())),
        (DisplayState::Switching { target, back: false }, DisplayState::Connected { .. }) => {
            Some(Event::Switched(server.or_else(|| Some(target.clone()))))
        }
        (DisplayState::Switching { target, back: true }, DisplayState::Connected { .. }) => {
            Some(Event::SwitchedBack(server.or_else(|| Some(target.clone()))))
        }
        (DisplayState::NotConnected, DisplayState::Blocked) => Some(Event::Blocked),
        (DisplayState::Blocked | DisplayState::ServiceUnreachableBlocked, DisplayState::NotConnected) => {
            Some(Event::Unblocked)
        }
        _ => None,
    }
}

/// Only with the switch on and while the window is not in front.
pub fn should_show(enabled: bool, front: bool) -> bool {
    enabled && !front
}

/// Country and place of a host, or `None` if it is not in the list (its name is never shown).
fn place(host: Option<&str>, countries: &[Country], language: Language) -> Option<String> {
    let host = host?;
    if streaming::describe_host(host).is_some() {
        return Some(streaming::short_label(host).render(language));
    }
    look::location_of(host, countries, language).map(|(_, label)| label)
}

/// Text of a notification, cleaned for the desktop: it reads the text as simple HTML, so `&`, `<` and `>` are
/// written as entities; no control characters; at most 200 characters.
pub fn body(event: &Event, language: Language, countries: &[Country]) -> String {
    let fill = |key: Key, host: &Option<String>, fallback: Key| match place(host.as_deref(), countries, language) {
        Some(label) => i18n::fill(i18n::text(language, key), &[label]),
        None => i18n::text(language, fallback).to_owned(),
    };
    let plain = match event {
        Event::Connected(host) => fill(Key::NotifyConnected, host, Key::StateConnectedTitle),
        Event::Switched(host) => fill(Key::NotifySwitched, host, Key::StateConnectedTitle),
        Event::SwitchedBack(host) => fill(Key::NotifySwitchedBack, host, Key::NotifyActionFailed),
        Event::Failed(host) => fill(Key::NotifyFailed, host, Key::NotifyActionFailed),
        Event::Disconnected => i18n::text(language, Key::NotifyDisconnected).to_owned(),
        Event::Lost { blocked: false } => i18n::text(language, Key::NotifyLost).to_owned(),
        Event::Lost { blocked: true } => i18n::text(language, Key::NotifyLostBlocked).to_owned(),
        Event::Blocked => i18n::text(language, Key::NotifyBlocked).to_owned(),
        Event::Unblocked => i18n::text(language, Key::NotifyUnblocked).to_owned(),
        Event::ActionFailed(message) => {
            let text = without_hosts(&message.render(language), countries, language);
            if look::contains_host_or_address(&text) {
                i18n::text(language, Key::NotifyActionFailed).to_owned()
            } else {
                text
            }
        }
    };
    escape(&text::printable(&plain, MAX_BODY))
}

/// Replaces every known host in `text` by its country and place, longest names first, and only where the name
/// stands on its own (not inside a longer name).
fn without_hosts(text: &str, countries: &[Country], language: Language) -> String {
    let mut hosts: Vec<String> = countries
        .iter()
        .flat_map(|country| &country.servers)
        .map(|server| server.host.clone())
        .chain(streaming::all().into_iter().map(|server| server.host))
        .collect();
    hosts.sort_by_key(|host| std::cmp::Reverse(host.len()));
    let mut result = text.to_owned();
    for host in hosts {
        let Some(label) = place(Some(&host), countries, language) else {
            continue;
        };
        result = replace_word(&result, &host, &label);
    }
    result
}

/// Replaces `word` where neither a letter, a digit, `-` nor `.` touches it on the left or a letter, a digit or
/// `-` on the right.
fn replace_word(text: &str, word: &str, by: &str) -> String {
    let part = |c: char| c.is_ascii_alphanumeric() || c == '-';
    let mut result = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(index) = rest.to_ascii_lowercase().find(&word.to_ascii_lowercase()) {
        let before = rest[..index].chars().next_back();
        let after = rest[index + word.len()..].chars().next();
        result.push_str(&rest[..index]);
        if before.is_some_and(|c| part(c) || c == '.') || after.is_some_and(part) {
            result.push_str(&rest[index..index + word.len()]);
        } else {
            result.push_str(by);
        }
        rest = &rest[index + word.len()..];
    }
    result.push_str(rest);
    result
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// The same event twice in a row is shown only once.
#[derive(Debug, Default)]
pub struct Dedup {
    last: Option<Event>,
}

impl Dedup {
    /// True if `event` differs from the one before.
    pub fn admit(&mut self, event: &Event) -> bool {
        if self.last.as_ref() == Some(event) {
            return false;
        }
        self.last = Some(event.clone());
        true
    }
}

/// How long after a choice in the menu its message still counts as the answer to it.
pub const MENU_FEEDBACK_TIME: Duration = Duration::from_secs(30);

/// How the answer to a choice in the menu reaches the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Feedback {
    /// As a desktop notification.
    Notify(Event),
    /// Notifications are switched off: the window comes back with the message.
    Reveal,
}

/// Where the answer can be seen at the moment of a display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Delivery {
    /// The switch "Show notifications".
    pub notifications: bool,
    /// The window is in front: the message can be seen there.
    pub front: bool,
    /// The window came back since the choice, because a start of the service ended with a reason; the message is
    /// in it.
    pub revealed: bool,
}

/// The answer owed to a choice in the menu: the first message of the status card set after the choice goes out
/// once, even if it has the same text as an earlier one. Without a choice in the menu messages of the window are
/// no notifications.
#[derive(Debug, Default)]
pub struct MenuFeedback {
    /// Time of the choice and the notice count of the monitor at that moment.
    owed: Option<(Instant, u64)>,
}

impl MenuFeedback {
    /// A choice in the menu started an action; `notice_count` is `Monitor::notice_count` right before it.
    pub fn chosen(&mut self, now: Instant, notice_count: u64) {
        self.owed = Some((now, notice_count));
    }

    /// The answer owed for this display, if any. `notice` and `notice_count` are the message of the status card
    /// and the count at the time of the display; `event` tells whether the display itself is reported (for
    /// example "Connected"), which then answers the choice when no message came. An answer that can already be
    /// seen (window in front or brought back) is settled without a second one.
    pub fn settle(
        &mut self,
        notice: Option<&Phrase>,
        notice_count: u64,
        event: bool,
        delivery: Delivery,
        now: Instant,
    ) -> Option<Feedback> {
        let (since, before) = self.owed?;
        if now.saturating_duration_since(since) >= MENU_FEEDBACK_TIME {
            self.owed = None;
            return None;
        }
        match notice {
            Some(text) if notice_count != before => {
                self.owed = None;
                if delivery.front || delivery.revealed {
                    None
                } else if delivery.notifications {
                    Some(Feedback::Notify(Event::ActionFailed(text.clone())))
                } else {
                    Some(Feedback::Reveal)
                }
            }
            _ => {
                if event {
                    self.owed = None;
                }
                None
            }
        }
    }
}

/// Sends one notification; `replaces` is the number of the previous one, so that the new one takes its place.
fn notify(connection: &Connection, replaces: u32, body: &str) -> zbus::Result<u32> {
    let mut hints: HashMap<&str, Value> = HashMap::new();
    hints.insert("desktop-entry", Value::from(crate::desktop::APP_ID));
    hints.insert("urgency", Value::U8(1));
    let actions: Vec<&str> = Vec::new();
    connection
        .call_method(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            Some("org.freedesktop.Notifications"),
            "Notify",
            &(APP_NAME, replaces, APP_ICON, APP_NAME, body, actions, hints, -1i32),
        )?
        .body()
        .deserialize()
}

fn run(receiver: &Receiver<String>) {
    let mut connection: Option<Connection> = None;
    let mut last_id = 0;
    for body in receiver {
        if connection.is_none() {
            connection = crate::presence::session().ok();
        }
        let Some(open) = connection.as_ref() else { continue };
        // A missing service or a time-out drops the notification without a message; next time connect anew.
        match notify(open, last_id, &body) {
            Ok(id) => last_id = id,
            Err(_) => connection = None,
        }
    }
}

/// Starts the thread `notifier`. Texts sent to the returned channel become notifications; the window thread
/// never waits for the desktop.
pub fn spawn() -> std::io::Result<Sender<String>> {
    let (sender, receiver) = mpsc::channel();
    thread::Builder::new()
        .name("notifier".to_owned())
        .spawn(move || run(&receiver))?;
    Ok(sender)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::servers::parse_list;

    const LIST: &[u8] = include_bytes!("../tests/fixtures/servers-2026-11-19.json");

    fn real() -> Vec<Country> {
        parse_list(LIST).unwrap()
    }

    fn shown(display: DisplayState) -> Shown {
        Shown {
            display: Some(display),
            connect_target: None,
        }
    }

    fn connecting(target: &str) -> Shown {
        Shown {
            display: Some(DisplayState::Connecting),
            connect_target: Some(target.to_owned()),
        }
    }

    fn connected(host: &str) -> Shown {
        shown(DisplayState::Connected {
            server: Some(host.to_owned()),
        })
    }

    fn switching(target: &str, back: bool) -> Shown {
        shown(DisplayState::Switching {
            target: target.to_owned(),
            back,
        })
    }

    #[test]
    fn ereignisse_nach_der_tabelle() {
        let at = "at.hideservers.net";
        let cases = [
            (connecting(at), connected(at), Some(Event::Connected(Some(at.into())))),
            (
                Shown {
                    display: Some(DisplayState::ConnectingBlocked),
                    connect_target: Some(at.into()),
                },
                shown(DisplayState::Connected { server: None }),
                Some(Event::Connected(Some(at.into()))),
            ),
            (
                shown(DisplayState::Disconnecting),
                shown(DisplayState::NotConnected),
                Some(Event::Disconnected),
            ),
            (
                connected(at),
                shown(DisplayState::NotConnected),
                Some(Event::Lost { blocked: false }),
            ),
            (
                connected(at),
                shown(DisplayState::Blocked),
                Some(Event::Lost { blocked: true }),
            ),
            (
                connecting(at),
                shown(DisplayState::NotConnected),
                Some(Event::Failed(Some(at.into()))),
            ),
            (
                connecting(at),
                shown(DisplayState::Blocked),
                Some(Event::Failed(Some(at.into()))),
            ),
            (
                switching(at, false),
                connected(at),
                Some(Event::Switched(Some(at.into()))),
            ),
            (
                switching(at, true),
                connected("de.hideservers.net"),
                Some(Event::SwitchedBack(Some("de.hideservers.net".into()))),
            ),
            (
                switching(at, false),
                shown(DisplayState::Blocked),
                Some(Event::Lost { blocked: true }),
            ),
            (
                shown(DisplayState::NotConnected),
                shown(DisplayState::Blocked),
                Some(Event::Blocked),
            ),
            (
                shown(DisplayState::Blocked),
                shown(DisplayState::NotConnected),
                Some(Event::Unblocked),
            ),
            (
                shown(DisplayState::ServiceUnreachableBlocked),
                shown(DisplayState::NotConnected),
                Some(Event::Unblocked),
            ),
        ];
        for (before, after, expected) in cases {
            assert_eq!(event(&before, &after), expected, "{before:?} → {after:?}");
        }
    }

    #[test]
    fn keine_ereignisse_fuer_zwischenschritte_und_neuverbinden() {
        let at = "at.hideservers.net";
        let cases = [
            (Shown::default(), connected(at)),
            (shown(DisplayState::Reapplying { back: false }), connected(at)),
            (shown(DisplayState::Reapplying { back: true }), connected(at)),
            (connected(at), shown(DisplayState::Reapplying { back: false })),
            (connected(at), switching("de.hideservers.net", false)),
            (switching(at, false), switching("de.hideservers.net", false)),
            (switching(at, false), switching(at, true)),
            (connected(at), connected(at)),
            (connected(at), shown(DisplayState::Disconnecting)),
            (shown(DisplayState::NotConnected), connecting(at)),
            (shown(DisplayState::NotConnected), shown(DisplayState::NotConnected)),
            (
                shown(DisplayState::NotConnected),
                shown(DisplayState::ServiceUnreachable),
            ),
            (shown(DisplayState::Blocked), shown(DisplayState::ConnectingBlocked)),
        ];
        for (before, after) in cases {
            assert_eq!(event(&before, &after), None, "{before:?} → {after:?}");
        }
    }

    #[test]
    fn nur_bei_eingeschaltetem_schalter_und_fenster_nicht_vorn() {
        assert!(should_show(true, false));
        assert!(!should_show(true, true));
        assert!(!should_show(false, false));
        assert!(!should_show(false, true));
    }

    #[test]
    fn texte_mit_land_und_ort() {
        let countries = real();
        let vienna = Some("at.hideservers.net".to_owned());
        assert_eq!(
            body(&Event::Connected(vienna.clone()), Language::De, &countries),
            "Verbunden: Österreich, Wien"
        );
        assert_eq!(
            body(&Event::Connected(vienna.clone()), Language::En, &countries),
            "Connected: Austria, Vienna"
        );
        assert_eq!(
            body(
                &Event::Switched(Some("streaming-jp.hideservers.net".into())),
                Language::De,
                &countries
            ),
            "Gewechselt zu Streaming Japan"
        );
        assert_eq!(
            body(&Event::Failed(vienna.clone()), Language::De, &countries),
            "Verbinden mit Österreich, Wien gescheitert"
        );
        assert_eq!(body(&Event::Disconnected, Language::De, &countries), "Getrennt");
        assert_eq!(body(&Event::Unblocked, Language::En, &countries), "Internet unblocked");
        // An unknown host is never named.
        assert_eq!(
            body(
                &Event::Connected(Some("x1.example.net".into())),
                Language::De,
                &countries
            ),
            "Verbunden"
        );
        assert_eq!(
            body(&Event::Failed(Some("x1.example.net".into())), Language::En, &countries),
            "Action failed. Details in the Celata window."
        );
        assert_eq!(body(&Event::Connected(None), Language::En, &countries), "Connected");
    }

    #[test]
    fn text_wird_bereinigt() {
        let raw = Event::ActionFailed(Phrase::raw("a & b <i>c</i>\n\u{1b}[31m"));
        assert_eq!(body(&raw, Language::En, &[]), "a &amp; b &lt;i&gt;c&lt;/i&gt;[31m");
        let long = Event::ActionFailed(Phrase::raw("x".repeat(500)));
        assert_eq!(body(&long, Language::En, &[]).chars().count(), 200);
    }

    #[test]
    fn meldung_nach_menue_aktion_ohne_hostnamen() {
        let countries = real();
        let message = Phrase::fill(
            Key::NoticeConnectRejected,
            [Phrase::raw(
                "server at.hideservers.net refused (free-de.hideservers.net)",
            )],
        );
        let text = body(&Event::ActionFailed(message), Language::De, &countries);
        assert!(text.contains("Österreich, Wien"), "{text}");
        assert!(!text.contains("hideservers"), "{text}");
        assert!(!look::contains_host_or_address(&text), "{text}");
        // An address or an unknown name: the fixed sentence.
        for raw in [
            "route via 192.0.2.10 failed",
            "unknown.example.org failed",
            "fe80::1 failed",
        ] {
            let text = body(&Event::ActionFailed(Phrase::raw(raw)), Language::De, &countries);
            assert_eq!(text, "Aktion gescheitert. Einzelheiten im Fenster von Celata.");
        }
        // A notice without host stays as it is.
        let text = body(
            &Event::ActionFailed(Phrase::key(Key::NoticeBestAlready)),
            Language::En,
            &countries,
        );
        assert_eq!(text, i18n::text(Language::En, Key::NoticeBestAlready));
    }

    #[test]
    fn ersetzen_nur_ganzer_namen() {
        assert_eq!(
            replace_word("de.x.net and free-de.x.net", "de.x.net", "DE"),
            "DE and free-de.x.net"
        );
        assert_eq!(replace_word("(DE.X.NET)", "de.x.net", "DE"), "(DE)");
        assert_eq!(replace_word("de.x.network", "de.x.net", "DE"), "de.x.network");
    }

    #[test]
    fn kein_hostname_und_keine_adresse_in_benachrichtigungen() {
        let countries = real();
        let mut hosts: Vec<Option<String>> = countries
            .iter()
            .flat_map(|country| &country.servers)
            .map(|server| Some(server.host.clone()))
            .collect();
        hosts.extend(streaming::all().into_iter().map(|server| Some(server.host)));
        hosts.push(Some("unknown.example.org".into()));
        hosts.push(Some("192.0.2.10".into()));
        hosts.push(None);
        for language in Language::ALL {
            for host in &hosts {
                for event in [
                    Event::Connected(host.clone()),
                    Event::Failed(host.clone()),
                    Event::Switched(host.clone()),
                    Event::SwitchedBack(host.clone()),
                    Event::ActionFailed(Phrase::raw(host.clone().unwrap_or_default())),
                ] {
                    let text = body(&event, language, &countries);
                    assert!(!look::contains_host_or_address(&text), "{event:?} {language:?}: {text}");
                }
            }
            for event in [
                Event::Disconnected,
                Event::Lost { blocked: false },
                Event::Lost { blocked: true },
                Event::Blocked,
                Event::Unblocked,
            ] {
                assert!(!look::contains_host_or_address(&body(&event, language, &countries)));
            }
        }
    }

    const SHOWN: Delivery = Delivery {
        notifications: true,
        front: false,
        revealed: false,
    };

    fn notify(text: &Phrase) -> Feedback {
        Feedback::Notify(Event::ActionFailed(text.clone()))
    }

    #[test]
    fn jede_wahl_im_menue_meldet_ihre_meldung_einmal() {
        let start = Instant::now();
        let text = Phrase::key(Key::NoticeBestAlready);
        let mut feedback = MenuFeedback::default();
        // Without a choice in the menu, messages of the window are no notifications.
        assert_eq!(feedback.settle(Some(&text), 1, false, SHOWN, start), None);
        // A message set after the choice goes out once, even with the text shown before.
        feedback.chosen(start, 1);
        assert_eq!(
            feedback.settle(Some(&text), 1, false, SHOWN, start),
            None,
            "old message"
        );
        assert_eq!(
            feedback.settle(Some(&text), 2, false, SHOWN, start + Duration::from_secs(1)),
            Some(notify(&text))
        );
        assert_eq!(
            feedback.settle(Some(&text), 2, false, SHOWN, start + Duration::from_secs(2)),
            None
        );
        // The same choice again: the same text once more.
        feedback.chosen(start + Duration::from_secs(3), 2);
        assert_eq!(
            feedback.settle(Some(&text), 3, false, SHOWN, start + Duration::from_secs(4)),
            Some(notify(&text))
        );
        // After 30 seconds the message no longer answers the choice.
        feedback.chosen(start, 3);
        assert_eq!(
            feedback.settle(Some(&text), 4, false, SHOWN, start + MENU_FEEDBACK_TIME),
            None
        );
        assert_eq!(
            feedback.settle(Some(&text), 5, false, SHOWN, start + Duration::from_secs(1)),
            None
        );
        // A reported display without a message answers the choice; a later message is no longer owed.
        feedback.chosen(start, 5);
        assert_eq!(feedback.settle(None, 5, false, SHOWN, start), None);
        assert_eq!(feedback.settle(None, 5, true, SHOWN, start), None);
        assert_eq!(feedback.settle(Some(&text), 6, false, SHOWN, start), None);
        // A failure with its own display event still reports its message.
        feedback.chosen(start, 6);
        assert_eq!(feedback.settle(Some(&text), 7, true, SHOWN, start), Some(notify(&text)));
    }

    #[test]
    fn rueckmeldung_kommt_dort_an_wo_sie_sichtbar_ist() {
        let start = Instant::now();
        let text = Phrase::key(Key::NoticeBestAlready);
        let mut feedback = MenuFeedback::default();
        // Notifications off: the window comes back with the message, each time.
        let off = Delivery {
            notifications: false,
            ..SHOWN
        };
        for count in 1..3 {
            feedback.chosen(start, count);
            assert_eq!(
                feedback.settle(Some(&text), count + 1, false, off, start),
                Some(Feedback::Reveal)
            );
            assert_eq!(feedback.settle(Some(&text), count + 2, false, off, start), None);
        }
        // Window in front: the message is there already.
        feedback.chosen(start, 10);
        let front = Delivery { front: true, ..SHOWN };
        assert_eq!(feedback.settle(Some(&text), 11, false, front, start), None);
        assert_eq!(feedback.settle(Some(&text), 12, false, SHOWN, start), None, "settled");
        // The window came back because a start of the service failed: no second answer.
        feedback.chosen(start, 20);
        let revealed = Delivery {
            revealed: true,
            ..SHOWN
        };
        assert_eq!(feedback.settle(Some(&text), 21, false, revealed, start), None);
        assert_eq!(feedback.settle(Some(&text), 22, false, SHOWN, start), None, "settled");
        // After 30 seconds nothing, also without notifications.
        feedback.chosen(start, 30);
        assert_eq!(
            feedback.settle(Some(&text), 31, false, off, start + MENU_FEEDBACK_TIME),
            None
        );
    }

    #[test]
    fn gleiches_ereignis_nur_einmal() {
        let mut dedup = Dedup::default();
        assert!(dedup.admit(&Event::Blocked));
        assert!(!dedup.admit(&Event::Blocked));
        assert!(dedup.admit(&Event::Unblocked));
        assert!(dedup.admit(&Event::Blocked));
        assert!(dedup.admit(&Event::Connected(Some("a".into()))));
        assert!(dedup.admit(&Event::Connected(Some("b".into()))));
    }
}
