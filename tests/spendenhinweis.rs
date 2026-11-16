// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! Checks the source code for the promises of the donation hint: only `src/donation.rs` knows the
//! Ko-fi link, and its program part contains neither network nor file nor environment access.
//! Reads the files like `tests/kontrast.rs` reads the colour scheme.

use std::fs;
use std::path::{Path, PathBuf};

const DONATION: &str = "src/donation.rs";
const TEST_MARKER: &str = "#[cfg(test)]";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// All `.rs` files below `dir`, recursively, as paths relative to the project directory.
fn rust_files(dir: &Path, found: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .expect("Ordner lesbar")
        .map(|entry| entry.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            rust_files(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            found.push(path.strip_prefix(root()).unwrap().to_path_buf());
        }
    }
}

/// Program part of `src/donation.rs`: everything before the first line with `#[cfg(test)]`. Aborts if
/// anything other than the test module follows after it.
fn program_part() -> String {
    let content = fs::read_to_string(root().join(DONATION)).expect("src/donation.rs lesbar");
    let lines: Vec<&str> = content.lines().collect();
    let Some(start) = lines.iter().position(|line| line.trim_start().starts_with(TEST_MARKER)) else {
        return content;
    };
    let rest: Vec<&str> = lines[start + 1..]
        .iter()
        .copied()
        .filter(|line| !line.trim().is_empty())
        .collect();
    assert_eq!(
        rest.first().map(|line| line.trim()),
        Some("mod tests {"),
        "Nach {TEST_MARKER} in {DONATION} muss direkt das Testmodul `mod tests` folgen"
    );
    let last = rest
        .iter()
        .rposition(|line| *line == "}")
        .expect("Testmodul endet mit `}`");
    assert_eq!(
        last,
        rest.len() - 1,
        "In {DONATION} darf nach dem Testmodul nichts mehr stehen (Programmcode gehört vor {TEST_MARKER})"
    );
    let unindented_closings = rest.iter().filter(|line| line.starts_with('}')).count();
    assert_eq!(
        unindented_closings, 1,
        "In {DONATION} steht nach {TEST_MARKER} mehr als das Testmodul (Programmcode gehört davor)"
    );
    lines[..start].join("\n")
}

fn findings(text: &str, forbidden: &[&str]) -> Vec<String> {
    text.lines()
        .enumerate()
        .filter_map(|(index, line)| {
            forbidden
                .iter()
                .find(|word| line.contains(*word))
                .map(|word| format!("Zeile {}: {word}", index + 1))
        })
        .collect()
}

#[test]
fn nur_donation_kennt_den_link() {
    let mut files = Vec::new();
    rust_files(&root().join("src"), &mut files);
    let elsewhere: Vec<String> = files
        .iter()
        .filter(|path| path.as_path() != Path::new(DONATION))
        .flat_map(|path| {
            let content = fs::read_to_string(root().join(path)).unwrap();
            findings(&content, &["ko-fi.com"])
                .into_iter()
                .map(move |hit| format!("{}: {hit}", path.display()))
        })
        .collect();
    assert!(
        elsewhere.is_empty(),
        "ko-fi.com steht außerhalb von {DONATION}:\n{}",
        elsewhere.join("\n")
    );
    assert!(
        files.iter().any(|path| path.as_path() == Path::new(DONATION)),
        "{DONATION} fehlt"
    );
}

/// Translations and user interface do not know the link either; it stays in exactly one place.
/// `src/i18n` is already checked by the test above.
#[test]
fn uebersetzungen_kennen_den_link_nicht() {
    fn files_in(dir: &Path, found: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).expect("Ordner lesbar") {
            let path = entry.unwrap().path();
            if path.is_dir() {
                files_in(&path, found);
            } else if path.extension().is_some_and(|ext| ext == "po" || ext == "slint") {
                found.push(path);
            }
        }
    }
    let mut files = Vec::new();
    files_in(&root().join("ui"), &mut files);
    assert!(
        files.iter().any(|path| path.extension().is_some_and(|ext| ext == "po")),
        "keine .po-Datei gefunden"
    );
    let hits: Vec<String> = files
        .iter()
        .flat_map(|path| {
            let content = fs::read_to_string(path).unwrap();
            findings(&content, &["ko-fi.com"])
                .into_iter()
                .map(move |hit| format!("{}: {hit}", path.display()))
        })
        .collect();
    assert!(
        hits.is_empty(),
        "ko-fi.com steht in der Oberfläche oder in Übersetzungen:\n{}",
        hits.join("\n")
    );
}

#[test]
fn programmteil_hat_keinen_netzwerkcode() {
    let hits = findings(
        &program_part(),
        &["ureq", "TcpStream", "UdpSocket", "ToSocketAddrs", "std::net"],
    );
    assert!(hits.is_empty(), "Netzwerkcode in {DONATION}:\n{}", hits.join("\n"));
}

#[test]
fn programmteil_liest_nichts_von_aussen_und_schreibt_nichts() {
    let hits = findings(&program_part(), &["env::var", "fs::"]);
    assert!(
        hits.is_empty(),
        "Datei- oder Umgebungszugriff in {DONATION}:\n{}",
        hits.join("\n")
    );
}

#[test]
fn programmteil_ist_nicht_leer() {
    // Protection against a boundary in the wrong place: the program part must contain the link.
    assert!(
        program_part().contains("pub const KOFI_URL"),
        "KOFI_URL fehlt im Programmteil von {DONATION}"
    );
}
