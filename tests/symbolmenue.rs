// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! Checks the source code for the promises of the menu at the tray icon and the work in the background: the menu,
//! notifications, the presence of the icon, closing, the start at login, the second start and `--unblock` send
//! nothing into the network and start no program; only the check for new versions asks GitHub, and only the
//! public repository is named. The session bus is the one of the user, never the system bus. Reads the files
//! like `tests/bester_standort.rs`.

use std::fs;
use std::path::{Path, PathBuf};

const TEST_MARKER: &str = "#[cfg(test)]";

/// Modules of this feature without any network access.
const WITHOUT_NETWORK: [&str; 9] = [
    "src/look.rs",
    "src/continent.rs",
    "src/notify.rs",
    "src/presence.rs",
    "src/background.rs",
    "src/autostart.rs",
    "src/instance.rs",
    "src/about.rs",
    "src/unblock.rs",
];

/// Of those, the ones that touch no file either.
const WITHOUT_FILES: [&str; 7] = [
    "src/look.rs",
    "src/continent.rs",
    "src/notify.rs",
    "src/presence.rs",
    "src/background.rs",
    "src/instance.rs",
    "src/about.rs",
];

/// Parsing an address with `std::net::IpAddr` is no network access; sockets and fetching are.
const NETWORK: &[&str] = &[
    "crate::net",
    "net::get",
    "ureq",
    "TcpStream",
    "TcpListener",
    "UdpSocket",
    "ToSocketAddrs",
];
const FILES: &[&str] = &[
    "fs::",
    "File::",
    "OpenOptions",
    "create_dir",
    "write_atomic",
    "read_limited",
    "read_to_string",
];
const PROCESSES: &[&str] = &["Command", "Stdio", "process::exit", "exec("];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(path: &str) -> String {
    fs::read_to_string(root().join(path)).unwrap_or_else(|_| panic!("{path} lesbar"))
}

/// Program part of a file: everything before `#[cfg(test)]`.
fn program_part(path: &str) -> String {
    let content = read(path);
    match content.find(TEST_MARKER) {
        Some(start) => content[..start].to_owned(),
        None => content,
    }
}

/// Lines with one of `words`, comments left out.
fn findings(path: &str, words: &[&str]) -> Vec<String> {
    program_part(path)
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim_start().starts_with("//"))
        .filter_map(|(index, line)| {
            words
                .iter()
                .find(|word| line.contains(*word))
                .map(|word| format!("{path}:{}: {word}: {}", index + 1, line.trim()))
        })
        .collect()
}

/// All files below `dir` with one of the extensions, sorted.
fn files(dir: &Path, extensions: &[&str], found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = entries.map(|entry| entry.unwrap().path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            files(&path, extensions, found);
        } else if path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| extensions.contains(&ext))
        {
            found.push(path);
        }
    }
}

fn relative(path: &Path) -> String {
    path.strip_prefix(root()).unwrap().to_string_lossy().into_owned()
}

#[test]
fn menue_und_hintergrund_ohne_netzwerk_und_ohne_prozesse() {
    let mut found = Vec::new();
    for path in WITHOUT_NETWORK {
        found.extend(findings(path, NETWORK));
        found.extend(findings(path, PROCESSES));
    }
    for path in WITHOUT_FILES {
        found.extend(findings(path, FILES));
    }
    assert!(found.is_empty(), "Unerlaubte Zugriffe:\n{}", found.join("\n"));
}

#[test]
fn nur_die_update_pruefung_fragt_github() {
    let mut sources = Vec::new();
    files(&root().join("src"), &["rs"], &mut sources);
    let mut found = Vec::new();
    for path in &sources {
        let name = relative(path);
        let program = program_part(&name);
        if program.contains("api.github.com") && name != "src/update.rs" {
            found.push(format!("{name}: api.github.com"));
        }
        if program.contains("github.com/derHouwer/celata") && name != "src/update.rs" && name != "src/about.rs" {
            found.push(format!("{name}: Adresse des Projekts"));
        }
        // The setup guide linked from the status card is the third address, only in `src/about.rs`.
        if program.contains("docs/manual-setup.md") && name != "src/about.rs" {
            found.push(format!("{name}: Adresse der Anleitung"));
        }
    }
    assert!(found.is_empty(), "{}", found.join("\n"));
    // The request is exactly one call of `net::get` with the fixed address and the limit.
    let update = program_part("src/update.rs");
    assert_eq!(update.matches("net::get(").count(), 1);
    assert!(update.contains("net::get(LATEST_API, MAX_ANSWER)"));
    let about = program_part("src/about.rs");
    assert_eq!(about.matches("https://").count(), 2, "Projektseite und Anleitung");
    assert!(
        about
            .contains("SETUP_GUIDE_URL: &str = \"https://github.com/derHouwer/celata/blob/main/docs/manual-setup.md\"")
    );
}

#[test]
fn sitzungsbus_nie_systembus() {
    let mut found = Vec::new();
    for path in ["src/notify.rs", "src/presence.rs", "src/instance.rs", "src/main.rs"] {
        found.extend(findings(path, &["Builder::system", "Connection::system", "::system()"]));
    }
    assert!(found.is_empty(), "{}", found.join("\n"));
    for path in ["src/presence.rs", "src/instance.rs"] {
        assert!(program_part(path).contains("Builder::session()"), "{path}");
    }
    // Notifications use the connection of `presence` (2 seconds limit).
    assert!(program_part("src/notify.rs").contains("presence::session()"));
    assert!(program_part("src/presence.rs").contains("method_timeout(TIMEOUT)"));
    assert!(program_part("src/instance.rs").contains("method_timeout(TIMEOUT)"));
}

#[test]
fn zweiter_start_bietet_nur_activate() {
    let instance = program_part("src/instance.rs");
    let start = instance.find("impl Activator").expect("Schnittstelle vorhanden");
    let block = &instance[start..];
    let end = block.find("\n}\n").expect("Ende der Schnittstelle");
    let methods = block[..end].matches("fn ").count();
    assert_eq!(methods, 1, "genau eine Methode: {}", &block[..end]);
    assert!(block[..end].contains("fn activate(&self)"));
}

/// Name of the working repository, put together so that this file does not contain it either.
const WORKING_REPOSITORY: &str = concat!("celata", "-dev");

#[test]
fn arbeitsrepository_wird_nirgends_genannt() {
    let mut paths = Vec::new();
    files(&root().join("src"), &["rs"], &mut paths);
    files(&root().join("ui"), &["slint", "po"], &mut paths);
    files(&root().join("docs"), &["md"], &mut paths);
    paths.push(root().join("README.md"));
    let found: Vec<String> = paths
        .iter()
        .filter(|path| {
            fs::read_to_string(path)
                .unwrap_or_default()
                .contains(WORKING_REPOSITORY)
        })
        .map(|path| relative(path))
        .collect();
    assert!(found.is_empty(), "Arbeitsrepository genannt in: {found:?}");
}

#[test]
fn menue_texte_kommen_aus_rust() {
    let tray = read("ui/tray.slint");
    assert!(!tray.contains("@tr("), "ui/tray.slint enthält @tr");
    assert!(!tray.contains("hideservers"), "ui/tray.slint enthält einen Hostnamen");
}

#[test]
fn menue_hat_so_viele_abschnitte_wie_rust_fuellt() {
    let tray = read("ui/tray.slint");
    let count = celata::look::MENU_SLOTS;
    for index in 0..count {
        assert!(tray.contains(&format!("k.slots[{index}].singles")), "Abschnitt {index}");
        assert!(tray.contains(&format!("k.slots[{index}].groups")), "Abschnitt {index}");
    }
    assert!(
        !tray.contains(&format!("k.slots[{count}]")),
        "mehr Abschnitte als MENU_SLOTS"
    );
    // The sections sit inside the loop over the continents; nothing reads them from the root any more.
    assert!(
        tray.contains("for k in root.continents: Menu"),
        "Schleife über die Erdteile"
    );
    assert!(!tray.contains("root.slots"), "root.slots darf nicht mehr vorkommen");
}
