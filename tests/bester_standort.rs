// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! Checks the source code for the promises of "Best location" (feature 013): determining it sends nothing
//! into the network, starts no program and writes no file; only `src/zone.rs` looks at the
//! system, and only at the time zone, the system language and the time zone database; the location of the
//! user is read only when connecting through "Best location" and never stored. Reads the
//! files like `tests/spendenhinweis.rs`.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const BEST: &str = "src/best.rs";
const ZONE: &str = "src/zone.rs";
const TEST_MARKER: &str = "#[cfg(test)]";

/// Building blocks of network access, processes and writing files.
const FORBIDDEN: &[&str] = &[
    "std::net",
    "crate::net",
    "ureq",
    "resolves",
    "Client",
    "TcpStream",
    "UdpSocket",
    "Command",
    "process::",
    "fs::write",
    "File::create",
    "OpenOptions",
    "create_dir",
    "remove_",
    "rename",
    "write_atomic",
    "set_var",
];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(path: &str) -> String {
    fs::read_to_string(root().join(path)).unwrap_or_else(|_| panic!("{path} lesbar"))
}

/// Program part of a file: everything before the line with `#[cfg(test)]`, which must be followed only by the
/// test module.
fn program_part(path: &str) -> String {
    let content = read(path);
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
        "Nach {TEST_MARKER} in {path} muss direkt das Testmodul folgen"
    );
    assert_eq!(
        rest.iter().filter(|line| line.starts_with('}')).count(),
        1,
        "In {path} steht nach {TEST_MARKER} mehr als das Testmodul"
    );
    assert_eq!(rest.last().copied(), Some("}"), "{path} endet nicht mit dem Testmodul");
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

/// String literals of a text (simple reader: `"…"` without escaped quotes, which these files do not use).
fn literals(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for line in text.lines() {
        // Comments do not count; `//` inside a literal does.
        let mut in_string = false;
        let mut end = line.len();
        for (index, c) in line.char_indices() {
            if c == '"' {
                in_string = !in_string;
            } else if c == '/' && !in_string && line[index + 1..].starts_with('/') {
                end = index;
                break;
            }
        }
        let code = &line[..end];
        // Every second part between quotes is inside a literal.
        found.extend(code.split('"').skip(1).step_by(2).map(str::to_owned));
    }
    found
}

#[test]
fn ermittlung_ohne_netzwerk_prozesse_und_schreiben() {
    for path in [BEST, ZONE] {
        let found = findings(&program_part(path), FORBIDDEN);
        assert!(found.is_empty(), "{path}: {found:?}");
    }
}

#[test]
fn rangfolge_liest_weder_dateien_noch_umgebung() {
    let found = findings(
        &program_part(BEST),
        &[
            "env::",
            "std::env",
            "fs::",
            "files::",
            "File",
            "Path",
            "read_link",
            "zone::",
        ],
    );
    assert!(found.is_empty(), "{BEST}: {found:?}");
}

#[test]
fn ort_liest_nur_die_genannten_dateien_und_variablen() {
    let program = program_part(ZONE);
    let literals = literals(&program);
    // Absolute paths: only the time zone and the database.
    let paths: BTreeSet<&str> = literals
        .iter()
        .map(String::as_str)
        .filter(|literal| literal.starts_with('/'))
        .collect();
    assert_eq!(
        paths,
        BTreeSet::from(["/etc/localtime", "/etc/timezone", "/usr/share/zoneinfo"]),
        "{literals:?}"
    );
    // Files in the database directory.
    let joined: BTreeSet<&str> = program
        .split(".join(\"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .collect();
    assert_eq!(joined, BTreeSet::from(["zone.tab", "zone1970.tab", "tzdata.zi"]));
    // Variables of the environment.
    let mut variables = BTreeSet::new();
    for call in ["env::var(\"", "variable(\""] {
        for rest in program.split(call).skip(1) {
            variables.insert(rest.split('"').next().unwrap_or_default());
        }
    }
    assert_eq!(variables, BTreeSet::from(["TZ", "LC_ALL", "LC_MESSAGES", "LANG"]));
    let found = findings(&program, &["var_os", "env::vars", "args", "home_dir", "HOME", "XDG_"]);
    assert!(found.is_empty(), "{ZONE}: {found:?}");
    // Reading goes through the bounded reader only; the link itself is only read, never followed into a file.
    assert_eq!(program.matches("read_link(").count(), 1);
    assert!(!program.contains("fs::read(") && !program.contains("read_to_string"));
}

/// Body of the top-level function `name` in `text`: from its line to the next line that is only `}`.
fn function_body<'a>(text: &'a str, name: &str) -> (usize, &'a str) {
    let start = text
        .find(&format!("\nfn {name}("))
        .unwrap_or_else(|| panic!("fn {name} fehlt"));
    let end = text[start..].find("\n}\n").map_or(text.len(), |end| start + end + 2);
    (start, &text[start..end])
}

#[test]
fn ort_wird_nur_beim_verbinden_ermittelt() {
    let main = read("src/main.rs");
    assert_eq!(
        main.matches("system_origin").count(),
        1,
        "genau eine Fundstelle in src/main.rs"
    );
    let (start, body) = function_body(&main, "start_connect");
    let index = main.find("system_origin").unwrap();
    assert!(
        index > start && index < start + body.len(),
        "zone::system_origin steht nicht im Rumpf von fn start_connect"
    );
    // No other file calls it.
    let mut files = Vec::new();
    rust_files(&root().join("src"), &mut files);
    for path in files {
        let relative = path.strip_prefix(root()).unwrap().to_string_lossy().into_owned();
        if relative == "src/main.rs" || relative == ZONE {
            continue;
        }
        let content = fs::read_to_string(&path).unwrap();
        assert!(!content.contains("system_origin"), "{relative}");
    }
}

#[test]
fn ermitteltes_wird_nicht_gespeichert() {
    for path in ["src/settings.rs", "src/favorites.rs"] {
        let found = findings(&read(path), &["Memory", "best_host", "system_origin", "Origin"]);
        assert!(found.is_empty(), "{path}: {found:?}");
    }
}

fn rust_files(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("Ordner lesbar") {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            found.push(path);
        }
    }
}

#[test]
fn leser_der_texte_arbeitet() {
    assert_eq!(literals("let a = \"x\"; let b = \"y\"; // \"z\""), ["x", "y"]);
    assert_eq!(literals("f(\"/etc/a\", \"b\")"), ["/etc/a", "b"]);
}
