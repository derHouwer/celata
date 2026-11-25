// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! Is the icon of Celata really in the system tray?
//!
//! Slint registers the icon only once, when the component is created, and gives up silently if the system
//! tray is not there yet. A thread therefore asks the session bus every 2 seconds whether the tray (the
//! `StatusNotifierWatcher`) exists and whether it lists the icon of this process. Celata creates its icon only
//! once the tray is there, and keeps running in the background only while the icon is shown.
//!
//! Only the session bus of the user; no network, no process, no file.

use std::thread;
use std::time::Duration;

use zbus::blocking::Connection;
use zbus::zvariant::OwnedValue;

/// Well-known name of the system tray on the session bus.
const WATCHER: &str = "org.kde.StatusNotifierWatcher";
const WATCHER_PATH: &str = "/StatusNotifierWatcher";
/// Start of the bus name under which the tray library registers the icon of a process:
/// `org.kde.StatusNotifierItem-<pid>-<counter>`.
const ITEM_PREFIX: &str = "org.kde.StatusNotifierItem-";

/// How often the thread asks.
pub const INTERVAL: Duration = Duration::from_secs(2);
/// Longest wait for an answer of the session bus.
pub const TIMEOUT: Duration = Duration::from_secs(2);

/// Whether the icon can be seen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TrayPresence {
    /// Not asked yet.
    #[default]
    Unknown,
    /// No system tray (or no session bus).
    Absent,
    /// The system tray is there, but does not list the icon of Celata (yet).
    WatcherOnly,
    /// The system tray lists the icon of this process.
    Shown,
}

impl TrayPresence {
    /// The system tray is there: the icon can be created.
    pub fn tray_available(self) -> bool {
        matches!(self, Self::WatcherOnly | Self::Shown)
    }
}

/// Decides from the answers of the session bus. `items` are the entries of `RegisteredStatusNotifierItems`,
/// each a bus name, optionally followed by an object path (`org.kde.StatusNotifierItem-42-1/StatusNotifierItem`).
/// Only an entry whose bus name belongs to `pid` counts; another process with a longer number that starts with
/// the same digits does not.
pub fn evaluate(watcher_owned: bool, items: &[String], pid: u32) -> TrayPresence {
    if !watcher_owned {
        return TrayPresence::Absent;
    }
    let own = format!("{ITEM_PREFIX}{pid}-");
    let shown = items.iter().any(|item| {
        let service = item.split('/').next().unwrap_or_default();
        service
            .strip_prefix(&own)
            .is_some_and(|counter| !counter.is_empty() && counter.chars().all(|c| c.is_ascii_digit()))
    });
    if shown {
        TrayPresence::Shown
    } else {
        TrayPresence::WatcherOnly
    }
}

/// A connection to the session bus with a time limit for every call.
pub fn session() -> zbus::Result<Connection> {
    zbus::blocking::connection::Builder::session()?
        .method_timeout(TIMEOUT)
        .build()
}

/// Asks once: is the tray there, and which icons does it list?
fn ask(connection: &Connection) -> zbus::Result<(bool, Vec<String>)> {
    let owned: bool = connection
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "NameHasOwner",
            &(WATCHER,),
        )?
        .body()
        .deserialize()?;
    if !owned {
        return Ok((false, Vec::new()));
    }
    // A watcher that cannot answer counts as a tray without the icon.
    let items = connection
        .call_method(
            Some(WATCHER),
            WATCHER_PATH,
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &(WATCHER, "RegisteredStatusNotifierItems"),
        )
        .and_then(|reply| reply.body().deserialize::<OwnedValue>())
        .ok()
        .and_then(|value| Vec::<String>::try_from(value).ok())
        .unwrap_or_default();
    Ok((true, items))
}

/// Starts the thread `presence`. `on_change` gets every new result (the first one as well); if it returns
/// `false`, the thread ends (the event loop of the window is gone). Without a session bus it reports `Absent`
/// and tries to connect again every 2 seconds.
pub fn spawn(on_change: impl Fn(TrayPresence) -> bool + Send + 'static) -> std::io::Result<()> {
    let pid = std::process::id();
    thread::Builder::new()
        .name("presence".to_owned())
        .spawn(move || {
            let mut last = TrayPresence::Unknown;
            let mut connection: Option<Connection> = None;
            loop {
                if connection.is_none() {
                    connection = session().ok();
                }
                let now = match connection.as_ref().map(ask) {
                    Some(Ok((owned, items))) => evaluate(owned, &items, pid),
                    Some(Err(_)) => {
                        // Connect anew next time; the bus may have restarted.
                        connection = None;
                        TrayPresence::Absent
                    }
                    None => TrayPresence::Absent,
                };
                if now != last {
                    last = now;
                    if !on_change(now) {
                        return;
                    }
                }
                thread::sleep(INTERVAL);
            }
        })
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(list: &[&str]) -> Vec<String> {
        list.iter().map(|item| (*item).to_owned()).collect()
    }

    #[test]
    fn without_watcher_the_icon_is_absent() {
        assert_eq!(evaluate(false, &[], 42), TrayPresence::Absent);
        assert_eq!(
            evaluate(
                false,
                &items(&["org.kde.StatusNotifierItem-42-1/StatusNotifierItem"]),
                42
            ),
            TrayPresence::Absent
        );
    }

    #[test]
    fn watcher_without_own_entry() {
        assert_eq!(evaluate(true, &[], 42), TrayPresence::WatcherOnly);
        assert_eq!(
            evaluate(true, &items(&[":1.67/StatusNotifierItem"]), 42),
            TrayPresence::WatcherOnly
        );
    }

    #[test]
    fn own_entry_in_the_form_plasma_lists_it() {
        // Form seen at the running desktop (research M1): bus name of the library, then the object path.
        let list = items(&[
            ":1.67/StatusNotifierItem",
            "org.kde.StatusNotifierItem-17794-1/StatusNotifierItem",
        ]);
        assert_eq!(evaluate(true, &list, 17794), TrayPresence::Shown);
    }

    #[test]
    fn own_entry_without_path_and_with_other_counter() {
        assert_eq!(
            evaluate(true, &items(&["org.kde.StatusNotifierItem-42-1"]), 42),
            TrayPresence::Shown
        );
        assert_eq!(
            evaluate(
                true,
                &items(&["org.kde.StatusNotifierItem-42-17/StatusNotifierItem"]),
                42
            ),
            TrayPresence::Shown
        );
    }

    #[test]
    fn other_processes_do_not_count() {
        let list = items(&["org.kde.StatusNotifierItem-43-1/StatusNotifierItem"]);
        assert_eq!(evaluate(true, &list, 42), TrayPresence::WatcherOnly);
        // 12 is a prefix of 123.
        let list = items(&["org.kde.StatusNotifierItem-123-1/StatusNotifierItem"]);
        assert_eq!(evaluate(true, &list, 12), TrayPresence::WatcherOnly);
        let list = items(&["org.kde.StatusNotifierItem-12-1/StatusNotifierItem"]);
        assert_eq!(evaluate(true, &list, 123), TrayPresence::WatcherOnly);
    }

    #[test]
    fn malformed_entries_do_not_count() {
        for entry in [
            "org.kde.StatusNotifierItem-42-",
            "org.kde.StatusNotifierItem-42-x",
            "org.kde.StatusNotifierItem-42",
            "x.org.kde.StatusNotifierItem-42-1",
            "/org.kde.StatusNotifierItem-42-1",
            "",
        ] {
            assert_eq!(
                evaluate(true, &items(&[entry]), 42),
                TrayPresence::WatcherOnly,
                "{entry}"
            );
        }
    }

    #[test]
    fn tray_available_only_with_watcher() {
        assert!(!TrayPresence::Unknown.tray_available());
        assert!(!TrayPresence::Absent.tray_available());
        assert!(TrayPresence::WatcherOnly.tray_available());
        assert!(TrayPresence::Shown.tray_available());
    }
}
