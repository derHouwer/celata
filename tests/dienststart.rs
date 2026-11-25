// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! Checks the promises of the start of the service in the source code and in the rule for polkit: only
//! `src/unit.rs` talks to systemd, it only ever starts the one unit, asks for a password only after the button,
//! and the rule allows exactly this start and nothing else.

use std::fs;
use std::path::{Path, PathBuf};

const RULE_FILE: &str = "packaging/polkit/49-celata.rules";
const UNIT: &str = "src/unit.rs";
const TEST_MARKER: &str = "#[cfg(test)]";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(relative: &str) -> String {
    fs::read_to_string(root().join(relative)).unwrap_or_else(|_| panic!("{relative} lesbar"))
}

fn rule() -> String {
    read(RULE_FILE)
}

/// All `.rs` files below `dir`, as paths relative to the project directory.
fn rust_files(dir: &Path, found: &mut Vec<String>) {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .expect("Ordner lesbar")
        .map(|entry| entry.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            rust_files(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            found.push(path.strip_prefix(root()).unwrap().to_string_lossy().into_owned());
        }
    }
}

fn sources() -> Vec<String> {
    let mut found = Vec::new();
    rust_files(&root().join("src"), &mut found);
    assert!(found.iter().any(|path| path == UNIT));
    found
}

/// A line without its `//` comment; `//` inside a string literal (for example in a web address) stays.
fn code_part(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut in_string = false;
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' if in_string => index += 1,
            b'"' => in_string = !in_string,
            b'/' if !in_string && bytes.get(index + 1) == Some(&b'/') => return &line[..index],
            _ => {}
        }
        index += 1;
    }
    line
}

/// The program part of a file (before `#[cfg(test)]`) without comments.
fn program_code(relative: &str) -> String {
    read(relative)
        .lines()
        .take_while(|line| !line.trim_start().starts_with(TEST_MARKER))
        .map(code_part)
        .collect::<Vec<_>>()
        .join("\n")
}

/// The body of `fn <name>` in `code`, up to the first line that closes at the start of the line.
fn function_body<'a>(code: &'a str, name: &str) -> &'a str {
    let start = code
        .find(&format!("fn {name}("))
        .unwrap_or_else(|| panic!("fn {name} fehlt"));
    let rest = &code[start..];
    let end = rest.find("\n}").map_or(rest.len(), |end| end + 2);
    &rest[..end]
}

// --- The rule ---

#[test]
fn regel_erlaubt_nur_den_start_dieses_dienstes() {
    let rule = rule();
    assert_eq!(rule.matches("polkit.addRule").count(), 1, "{rule}");
    assert!(!rule.contains("addAdminRule"));
    assert_eq!(rule.matches("polkit.Result.YES").count(), 1);
    for forbidden in ["polkit.Result.NO", "AUTH_SELF", "AUTH_ADMIN", "_KEEP", "||"] {
        assert!(!rule.contains(forbidden), "{forbidden}");
    }
    // All conditions stand in the one `if`, joined with `&&`.
    let condition_start = rule.find("if (").expect("if");
    let condition_end = rule[condition_start..].find(") {").expect("Ende der Bedingung") + condition_start;
    let condition = &rule[condition_start + 4..condition_end];
    let unit = format!("action.lookup(\"unit\") === \"{}\"", celata::state::SERVICE_UNIT);
    let parts: Vec<String> = condition
        .split("&&")
        .map(|part| part.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect();
    assert_eq!(
        parts,
        [
            "action.id === \"org.freedesktop.systemd1.manage-units\"",
            unit.as_str(),
            "action.lookup(\"verb\") === \"start\"",
            "subject.local",
            "subject.active",
            "subject.isInGroup(\"celata\")",
        ],
        "{condition}"
    );
    assert_eq!(rule.matches("if (").count(), 1);
    assert_eq!(rule.matches("return").count(), 1);
}

#[test]
fn regel_hat_lizenzkopf_und_englische_kommentare() {
    let rule = rule();
    assert!(rule.starts_with("// SPDX-License-Identifier: GPL-3.0-only\n// Copyright (C) 2026 derHouwer\n"));
    for line in rule.lines().filter(|line| line.trim_start().starts_with("//")) {
        assert!(!line.chars().any(|c| "äöüÄÖÜß".contains(c)), "{line}");
        assert!(!line.contains("FR-") && !line.contains("SC-"), "{line}");
    }
    assert!(rule.ends_with('\n'));
}

#[test]
fn eingebaute_regel_ist_die_datei() {
    assert_eq!(celata::unit::RULE, rule());
}

// --- The source code ---

#[test]
fn celata_stoppt_den_dienst_nie() {
    let forbidden = [
        "StopUnit",
        "RestartUnit",
        "TryRestartUnit",
        "ReloadUnit",
        "ReloadOrRestartUnit",
        "ReloadOrTryRestartUnit",
        "KillUnit",
        "ResetFailedUnit",
        "EnableUnitFiles",
        "DisableUnitFiles",
        "MaskUnitFiles",
        "UnmaskUnitFiles",
        "SetUnitProperties",
        "StartTransientUnit",
        "Subscribe",
    ];
    let mut found = Vec::new();
    for path in sources() {
        let code = read(&path).lines().map(code_part).collect::<Vec<_>>().join("\n");
        for word in forbidden {
            if code.contains(word) {
                found.push(format!("{path}: {word}"));
            }
        }
    }
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn nur_unit_spricht_den_systembus_an() {
    for path in sources() {
        let code = program_code(&path);
        let opens = code.contains("Builder::system") || code.contains("Connection::system");
        assert_eq!(opens, path == UNIT, "{path}");
    }
}

#[test]
fn passwortabfrage_nur_nach_dem_knopf() {
    let mut total = 0;
    for path in sources() {
        total += program_code(&path).matches("AllowInteractiveAuth").count();
    }
    assert_eq!(total, 1, "AllowInteractiveAuth genau einmal");
    let unit = program_code(UNIT);
    assert!(function_body(&unit, "start_unit_interactive").contains("AllowInteractiveAuth"));
    // Called only from the button "Start service" in the window.
    for path in sources() {
        let calls = program_code(&path).matches("start_unit_interactive(").count();
        let expected = usize::from(path == UNIT || path == "src/main.rs");
        assert_eq!(calls, expected, "{path}");
    }
    let main = program_code("src/main.rs");
    let caller = function_body(&main, "start_service_interactive");
    assert!(caller.contains("unit::start_unit_interactive("), "{caller}");
    assert_eq!(
        main.matches("start_service_interactive(").count(),
        2,
        "Definition und ein Aufruf"
    );
    let button = main.find("on_start_service_clicked(").expect("Knopf verbunden");
    let call = main
        .match_indices("start_service_interactive(")
        .map(|(index, _)| index)
        .find(|index| !main[..*index].ends_with("fn "))
        .expect("Aufruf");
    assert!(call > button && call - button < 300, "Aufruf nur aus dem Knopf");
}

#[test]
fn unit_startet_keine_programme_und_nutzt_kein_netz() {
    let code = program_code(UNIT);
    let without_command = code.replace(function_body(&code, "rule_command"), "");
    for forbidden in [
        "sudo",
        "pkexec",
        "systemctl",
        "std::process",
        "Command::new",
        "TcpStream",
        "UdpSocket",
        "net::",
        "ureq",
        "fs::write",
        "File::create",
        "OpenOptions",
        "unsafe",
        "libc",
    ] {
        assert!(!without_command.contains(forbidden), "{forbidden}");
    }
    // Read are only these two files.
    assert_eq!(code.matches("read_to_string").count(), 1);
    assert!(code.contains("\"/proc/self/status\"") && code.contains("\"/etc/group\""));
}

#[test]
fn dienstname_nur_an_einer_stelle() {
    let name = "celata-hide.me.service";
    for path in sources() {
        let code = program_code(&path);
        for line in code.lines().filter(|line| line.contains(name)) {
            let allowed = (path == "src/state.rs" && line.trim_start().starts_with("pub const "))
                || ((path == "src/i18n/en.rs" || path == "src/i18n/de.rs") && line.contains("Key::CliEmergency"));
            assert!(allowed, "{path}: {line}");
        }
    }
    // Every call in `src/main.rs` names the unit through the constant.
    let main = program_code("src/main.rs");
    for call in ["unit::read_unit(", "unit::start_unit(", "unit::start_unit_interactive("] {
        for (index, _) in main.match_indices(call) {
            let argument = &main[index + call.len()..];
            assert!(argument.starts_with("state::SERVICE_UNIT"), "{call}");
        }
    }
}

#[test]
fn schliessen_und_aufheben_starten_nichts() {
    let main = program_code("src/main.rs");
    for function in [
        "close_app",
        "request_close",
        "start_disconnect",
        "finish_disconnect",
        "start_unblock",
    ] {
        let body = function_body(&main, function);
        assert!(!body.contains("unit::"), "{function}");
        assert!(!body.contains("service_start"), "{function}");
    }
    // `celata --unblock` uses nothing of the start.
    let unblock = program_code("src/unblock.rs");
    assert!(!unblock.contains("unit::") && !unblock.contains("crate::unit"));
}

#[test]
fn polkit_rule_option_gibt_die_regel_aus() {
    assert_eq!(
        celata::cli::parse(["celata", "--polkit-rule"]),
        celata::cli::Command::PolkitRule
    );
    let main = program_code("src/main.rs");
    let arm = main.find("cli::Command::PolkitRule").expect("Zweig für --polkit-rule");
    let arm = &main[arm..arm + 200];
    assert!(arm.contains("unit::RULE"), "{arm}");
}
