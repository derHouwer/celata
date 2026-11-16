// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! Release guards: facts that must hold for every published state of Celata.
//!
//! The tests read project files as plain text (no extra crates) and compare them with each other and with
//! constants of the program, so that version, licence, service unit, desktop entry and documentation cannot
//! drift apart unnoticed.
#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

/// The version is frozen until Celata is published. Remove this guard with the first release after that.
const FROZEN_VERSION: &str = "0.8.1";
const LICENSE_ID: &str = "GPL-3.0-only";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(relative: &str) -> String {
    fs::read_to_string(root().join(relative)).unwrap_or_else(|error| panic!("{relative} is not readable: {error}"))
}

/// Lines of one TOML table (`[name]`), without comments and blank lines.
fn toml_table(content: &str, name: &str) -> Vec<String> {
    let header = format!("[{name}]");
    content
        .lines()
        .map(str::trim)
        .skip_while(|line| *line != header)
        .skip(1)
        .take_while(|line| !line.starts_with('['))
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

/// Value of `key = "value"` in a list of TOML lines.
fn toml_string(lines: &[String], key: &str) -> Option<String> {
    lines.iter().find_map(|line| {
        let (name, value) = line.split_once('=')?;
        (name.trim() == key).then(|| value.trim().trim_matches('"').to_owned())
    })
}

/// The one place that defines the version: `[workspace.package]` in `Cargo.toml`.
fn workspace_version() -> String {
    toml_string(&toml_table(&read("Cargo.toml"), "workspace.package"), "version")
        .expect("Cargo.toml defines version in [workspace.package]")
}

#[test]
fn version_is_frozen_until_release() {
    assert_eq!(
        workspace_version(),
        FROZEN_VERSION,
        "the version stays {FROZEN_VERSION} until Celata is published"
    );
    assert_eq!(celata::cli::VERSION, FROZEN_VERSION);
    assert_eq!(celata::cli::version_line(), format!("celata {FROZEN_VERSION}"));
}

#[test]
fn both_packages_inherit_version_and_license() {
    for manifest in ["Cargo.toml", "crates/celata-ui/Cargo.toml"] {
        let package = toml_table(&read(manifest), "package");
        for key in ["version", "license", "edition", "rust-version", "publish"] {
            assert!(
                package
                    .iter()
                    .any(|line| line.replace(' ', "") == format!("{key}.workspace=true")),
                "{manifest}: `{key}` must be inherited with `{key}.workspace = true`"
            );
            assert!(
                toml_string(&package, key).is_none(),
                "{manifest}: `{key}` must not be set in [package], only in [workspace.package]"
            );
        }
    }
}

#[test]
fn license_is_gpl_3_only() {
    let workspace = toml_table(&read("Cargo.toml"), "workspace.package");
    assert_eq!(toml_string(&workspace, "license").as_deref(), Some(LICENSE_ID));
}

#[test]
fn license_file_holds_the_gpl_text() {
    let license = read("LICENSE");
    let mut lines = license.lines().map(str::trim).filter(|line| !line.is_empty());
    assert_eq!(lines.next(), Some("GNU GENERAL PUBLIC LICENSE"));
    assert_eq!(lines.next(), Some("Version 3, 29 June 2007"));
    // Complete text: it ends with the note on the Lesser GPL, and the warranty disclaimer is there.
    assert!(
        license.contains("15. Disclaimer of Warranty."),
        "LICENSE lacks the warranty disclaimer"
    );
    assert!(
        license.contains("END OF TERMS AND CONDITIONS"),
        "LICENSE lacks the end of the terms"
    );
    assert!(
        license
            .trim_end()
            .ends_with("<http://www.gnu.org/philosophy/why-not-lgpl.html>."),
        "LICENSE is cut off"
    );
}

#[test]
fn license_check_has_no_placeholders() {
    let deny = read("deny.toml");
    let licenses = toml_table(&deny, "licenses");
    assert!(
        !deny.contains("Royalty-free"),
        "deny.toml must not allow the Slint royalty-free licence any more"
    );
    assert!(
        !licenses.iter().any(|line| line.starts_with("private")),
        "deny.toml must not skip Celata as private"
    );
    assert!(
        licenses.iter().any(|line| line.contains(&format!("\"{LICENSE_ID}\""))),
        "deny.toml must allow {LICENSE_ID}"
    );
}

/// Name and version of every third-party package in `Cargo.lock`.
fn locked_packages() -> BTreeSet<(String, String)> {
    let lock = read("Cargo.lock");
    let value = |line: &str, key: &str| {
        Some(
            line.strip_prefix(key)?
                .strip_prefix(" = \"")?
                .strip_suffix('"')?
                .to_owned(),
        )
    };
    let mut lines = lock.lines();
    let mut packages = BTreeSet::new();
    while let Some(line) = lines.next() {
        if line == "[[package]]" {
            let name = lines
                .next()
                .and_then(|line| value(line, "name"))
                .expect("name follows [[package]]");
            let version = lines
                .next()
                .and_then(|line| value(line, "version"))
                .expect("version follows name");
            if !["celata", "celata-ui"].contains(&name.as_str()) {
                packages.insert((name, version));
            }
        }
    }
    packages
}

/// The cells of every table row below the heading `## heading`, up to the next heading of that level.
fn table_rows(content: &str, heading: &str) -> Vec<Vec<String>> {
    let start = format!("## {heading}");
    content
        .lines()
        .skip_while(|line| *line != start)
        .skip(1)
        .take_while(|line| !line.starts_with("## "))
        .filter(|line| line.starts_with("| ") && !line.starts_with("| Crate ") && !line.starts_with("| Name "))
        .map(|line| {
            line.trim_matches('|')
                .split(" | ")
                .map(|cell| cell.trim().to_owned())
                .collect()
        })
        .collect()
}

#[test]
fn third_party_licenses_account_for_every_locked_package() {
    let content = read("THIRD-PARTY-LICENSES.md");
    let names = |heading: &str| -> Vec<(String, String)> {
        table_rows(&content, heading)
            .into_iter()
            .map(|row| (row[0].clone(), row[1].clone()))
            .collect()
    };
    let linux = names("Rust crates in the Linux build");
    let other = names("Not part of the Linux build");
    assert!(
        linux.len() > 300,
        "the list of the Linux build looks cut off: {} rows",
        linux.len()
    );
    let listed: BTreeSet<_> = linux.iter().chain(&other).cloned().collect();
    assert_eq!(listed.len(), linux.len() + other.len(), "a package is listed twice");
    let locked = locked_packages();
    let missing: Vec<_> = locked.difference(&listed).collect();
    let stale: Vec<_> = listed.difference(&locked).collect();
    assert!(
        missing.is_empty() && stale.is_empty(),
        "THIRD-PARTY-LICENSES.md does not match Cargo.lock; regenerate it with packaging/third-party-licenses.py.\nmissing: {missing:?}\nstale: {stale:?}"
    );
}

#[test]
fn third_party_licenses_give_a_text_for_every_component() {
    let content = read("THIRD-PARTY-LICENSES.md");
    let texts = content.lines().filter(|line| line.starts_with("### Text ")).count();
    assert!(texts > 0);
    let mut rows = table_rows(&content, "Rust crates in the Linux build");
    let artwork = table_rows(&content, "Artwork");
    for (name, version, license) in [("flag-icons", "7.6.0", "MIT"), ("Lucide", "1.70.0", "ISC")] {
        assert!(
            artwork.iter().any(|row| row[..3] == [name, version, license]),
            "{name} {version} ({license}) is missing under Artwork"
        );
    }
    rows.extend(artwork);
    for row in rows {
        assert!(!row[2].is_empty(), "{} {} names no license", row[0], row[1]);
        let numbers: Vec<usize> = row[3]
            .split("](#text-")
            .skip(1)
            .filter_map(|rest| rest.split(')').next()?.parse().ok())
            .collect();
        assert!(!numbers.is_empty(), "{} {} links no license text", row[0], row[1]);
        assert!(
            numbers.iter().all(|number| (1..=texts).contains(number)),
            "{} {} links a missing text",
            row[0],
            row[1]
        );
    }
}

const SERVICE_FILE: &str = "packaging/systemd/celata-hide.me.service";
const ARCH_SERVICE_FILE: &str = "packaging/arch/celata-hide.me.service";

/// Hardening options of the unit as it was before version 0.8.1. They may be tightened, never dropped.
const BASE_HARDENING: [&str; 13] = [
    "ProtectSystem=strict",
    "ProtectHome=true",
    "ProtectKernelTunables=true",
    "ProtectKernelModules=true",
    "ProtectControlGroups=true",
    "PrivateTmp=true",
    "SystemCallArchitectures=native",
    "SystemCallFilter=~@module",
    "SystemCallFilter=~@mount",
    "MemoryDenyWriteExecute=true",
    "NoNewPrivileges=true",
    "MemoryMax=64M",
    "LimitNOFILE=64",
];

/// Lines of a unit, desktop entry or similar file without comments and blank lines.
fn settings_lines(content: &str) -> Vec<&str> {
    content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect()
}

/// The service is reachable only through the group-protected path socket and never connects on its own.
#[test]
fn service_unit_keeps_its_guarantees() {
    for file in [SERVICE_FILE, ARCH_SERVICE_FILE] {
        let content = read(file);
        let lines = settings_lines(&content);
        let has = |line: &str| lines.contains(&line);
        let socket = celata::daemon::DEFAULT_SOCKET;
        assert_eq!(socket, "/run/hide.me/control.sock");

        let exec: Vec<&str> = lines
            .iter()
            .filter(|line| line.starts_with("ExecStart="))
            .copied()
            .collect();
        assert_eq!(exec.len(), 1, "{file}: exactly one ExecStart");
        assert!(
            exec[0].ends_with(&format!("/hide.me --caddr {socket} service")),
            "{file}: {}",
            exec[0]
        );
        assert_eq!(
            content.matches("--caddr").count(),
            1,
            "{file}: exactly one control address"
        );
        assert!(!exec[0].contains('@'), "{file}: no abstract socket");
        assert!(
            !exec[0].contains(" connect"),
            "{file}: the service must never connect on its own"
        );
        // No control address with a port, and no server: the only arguments are the socket and the mode.
        assert_eq!(exec[0].split_whitespace().count(), 4, "{file}: {}", exec[0]);

        assert!(has("RuntimeDirectory=hide.me"), "{file}: RuntimeDirectory");
        assert!(
            has(&format!("ExecStartPost=/usr/bin/chgrp celata {socket}")),
            "{file}: group of the socket"
        );
        assert!(
            has(&format!("ExecStartPost=/usr/bin/chmod 0660 {socket}")),
            "{file}: mode of the socket"
        );

        let install: Vec<&str> = lines
            .iter()
            .skip_while(|line| **line != "[Install]")
            .skip(1)
            .copied()
            .collect();
        assert_eq!(install, ["WantedBy=multi-user.target"], "{file}: [Install]");
        assert!(
            !lines
                .iter()
                .any(|line| line.starts_with("Also=") || line.starts_with("Alias=")),
            "{file}"
        );

        for option in BASE_HARDENING {
            assert!(has(option), "{file}: hardening option {option} is gone");
        }
        // Write access as in the unit shipped with the client: its directory, /etc and the directory of
        // systemd-resolved. Access to the single file /etc/resolv.conf does not survive the system replacing
        // that file, and the client then cannot connect. Nothing else may become writable.
        let writable: Vec<&str> = lines
            .iter()
            .filter_map(|line| line.strip_prefix("ReadWritePaths="))
            .collect();
        assert_eq!(writable.len(), 1, "{file}: exactly one ReadWritePaths");
        let paths: Vec<&str> = writable[0].split_whitespace().collect();
        assert_eq!(paths.len(), 3, "{file}: {paths:?}");
        assert!(paths[0].ends_with("/hide.me"), "{file}: {paths:?}");
        assert_eq!(paths[1..], ["/etc", "-/run/systemd/resolve"], "{file}: {paths:?}");
        assert!(has("ProtectSystem=strict"), "{file}: everything else stays read-only");
    }
    assert_eq!(celata::state::SERVICE_UNIT, "celata-hide.me.service");
    assert!(
        SERVICE_FILE.ends_with(celata::state::SERVICE_UNIT) && ARCH_SERVICE_FILE.ends_with(celata::state::SERVICE_UNIT)
    );
    assert_eq!(settings_lines(&read("packaging/sysusers/celata.conf")), ["g celata -"]);
}

/// The Arch unit is the main unit for the file locations of the AUR package of the hide.me client.
#[test]
fn arch_service_unit_differs_in_three_lines_only() {
    let (main, arch) = (read(SERVICE_FILE), read(ARCH_SERVICE_FILE));
    assert_eq!(main.lines().count(), arch.lines().count());
    let different: Vec<(&str, &str)> = main
        .lines()
        .zip(arch.lines())
        .filter(|(main, arch)| main != arch)
        .collect();
    assert_eq!(
        different,
        [
            ("WorkingDirectory=/opt/hide.me", "WorkingDirectory=/etc/hide.me"),
            (
                "ExecStart=/opt/hide.me/hide.me --caddr /run/hide.me/control.sock service",
                "ExecStart=/usr/bin/hide.me --caddr /run/hide.me/control.sock service"
            ),
            (
                "ReadWritePaths=/opt/hide.me /etc -/run/systemd/resolve",
                "ReadWritePaths=/etc/hide.me /etc -/run/systemd/resolve"
            ),
        ]
    );
}

/// The packaged desktop entry and the one Celata writes itself describe the same program.
#[test]
fn packaged_desktop_entry_matches_the_generated_one() {
    let packaged = read("packaging/desktop/celata.desktop");
    let generated = celata::desktop::entry(std::path::Path::new("/usr/bin/celata"));
    let compared = |content: &str| -> Vec<String> {
        content
            .lines()
            .filter(|line| {
                let key = line.split('=').next().unwrap_or_default();
                ["Type", "Name", "Icon", "Terminal", "Categories", "StartupWMClass"].contains(&key)
                    || key == "Comment"
                    || key.starts_with("Comment[")
            })
            .map(str::to_owned)
            .collect()
    };
    assert_eq!(compared(&packaged), compared(&generated));
    assert!(compared(&packaged).len() >= 7, "{packaged}");
    let lines = settings_lines(&packaged);
    assert_eq!(lines[0], "[Desktop Entry]");
    assert!(
        lines.contains(&"Exec=celata"),
        "the packaged entry starts the program from the search path"
    );
    assert!(
        !packaged.contains(celata::desktop::MARKER),
        "the packaged entry belongs to the package, not to Celata"
    );
    assert_eq!(
        lines.len(),
        generated.lines().count() - 1,
        "no further keys:\n{packaged}"
    );
}

/// Every place outside Cargo that names the version follows `Cargo.toml`.
#[test]
fn version_is_the_same_everywhere() {
    let version = workspace_version();
    let first_with = |file: &str, prefix: &str| -> String {
        read(file)
            .lines()
            .find(|line| line.starts_with(prefix))
            .unwrap_or_else(|| panic!("{file}: no line starts with {prefix:?}"))
            .to_owned()
    };
    assert_eq!(
        first_with("packaging/debian/changelog", "celata ("),
        format!("celata ({version}-1) unstable; urgency=medium")
    );
    assert_eq!(
        first_with("packaging/rpm/celata.spec", "Version:")
            .split_whitespace()
            .nth(1),
        Some(version.as_str())
    );
    // One .rpm for every supported Fedora release: no distribution tag in the file name.
    assert!(first_with("packaging/rpm/celata.spec", "Release:").ends_with(" 1"));
    assert_eq!(
        first_with("packaging/arch/PKGBUILD", "pkgver="),
        format!("pkgver={version}")
    );
    assert_eq!(first_with("packaging/arch/PKGBUILD", "pkgrel="), "pkgrel=1");
    assert!(
        first_with(
            "packaging/desktop/io.github.derhouwer.celata.metainfo.xml",
            "    <release "
        )
        .contains(&format!("version=\"{version}\"")),
        "the newest release in the metainfo is not {version}"
    );
    assert!(
        first_with("CHANGELOG.md", "## [").starts_with(&format!("## [{version}]")),
        "the top entry of CHANGELOG.md is not {version}"
    );
    for file in [
        "packaging/debian/copyright",
        "packaging/rpm/celata.spec",
        "packaging/arch/PKGBUILD",
    ] {
        assert!(read(file).contains(LICENSE_ID), "{file} does not name {LICENSE_ID}");
    }
    for file in ["packaging/debian/control", "packaging/rpm/celata.spec"] {
        let flat = read(file).split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(
            flat.contains("Requires the hide.me command-line client (not included) and a hide.me account."),
            "{file}: the description must name the hide.me client as a requirement"
        );
        assert!(
            flat.contains("Wayland sessions are supported; X11 is enabled but untested."),
            "{file}"
        );
    }
    let metainfo = read("packaging/desktop/io.github.derhouwer.celata.metainfo.xml");
    assert!(metainfo.contains("<project_license>GPL-3.0-only</project_license>"));
    assert!(metainfo.contains(
        "Requires the hide.me command-line client (not included) and a hide.me account. Wayland sessions are \
         supported; X11 is enabled but untested."
    ));
    assert!(
        !metainfo.to_lowercase().contains("ko-fi"),
        "no donation link in the metainfo"
    );
}

/// The documentation every reader of the repository sees. All of it is English.
const DOCUMENTS: [&str; 17] = [
    "README.md",
    "LICENSE",
    "CHANGELOG.md",
    "CONTRIBUTING.md",
    "CODE_OF_CONDUCT.md",
    "SECURITY.md",
    "THIRD-PARTY-LICENSES.md",
    ".github/ISSUE_TEMPLATE/bug_report.yml",
    ".github/ISSUE_TEMPLATE/feature_request.yml",
    ".github/ISSUE_TEMPLATE/config.yml",
    "docs/dns-leak.md",
    "docs/MAINTAINING.md",
    "docs/manual-setup.md",
    "docs/packaging.md",
    "docs/ARCHITECTURE.md",
    "docs/REVIEW-GUIDE.md",
    "packaging/desktop/io.github.derhouwer.celata.metainfo.xml",
];

/// Documents written for this project (not license texts of others, which may name anything).
fn own_documents() -> impl Iterator<Item = (&'static str, String)> {
    DOCUMENTS
        .into_iter()
        .filter(|file| !["LICENSE", "THIRD-PARTY-LICENSES.md", "CODE_OF_CONDUCT.md"].contains(file))
        .map(|file| (file, read(file)))
}

#[test]
fn documentation_is_complete() {
    for file in DOCUMENTS {
        assert!(!read(file).trim().is_empty(), "{file} is empty");
    }
    let readme = read("README.md");
    for heading in [
        "## Status",
        "## Internet blocked? Emergency exit",
        "## Features",
        "## Requirements",
        "## Tested on",
        "## Install",
        "## Set up after installing",
        "## First steps",
        "## DNS leaks",
        "## Known limitations",
        "## Updating and uninstalling",
        "## Troubleshooting",
        "## Building from source",
        "## Security",
        "## Feedback",
        "## License and credits",
        "## Support",
    ] {
        assert!(
            readme.lines().any(|line| line == heading),
            "README.md lacks the heading {heading:?}"
        );
    }
    // The way out of a blocked connection stands before the installation, so that it has been read in time.
    let position = |heading: &str| readme.find(heading).expect("heading exists");
    assert!(position("## Internet blocked? Emergency exit") < position("## Install"));
    assert!(read("docs/dns-leak.md").starts_with("# DNS leaks\n"));
    // The three screenshots: PNG files in the size of the window, referenced by the README.
    for image in ["overview", "locations", "settings"] {
        let path = format!("docs/images/{image}.png");
        let data = fs::read(root().join(&path)).unwrap_or_else(|error| panic!("{path} is not readable: {error}"));
        assert!(
            data.starts_with(b"\x89PNG\r\n\x1a\n") && &data[12..16] == b"IHDR",
            "{path} is not a PNG file"
        );
        assert_eq!(
            (&data[16..20], &data[20..24]),
            (&880u32.to_be_bytes()[..], &780u32.to_be_bytes()[..]),
            "{path}"
        );
        assert!(readme.contains(&format!("({path})")), "README.md does not show {path}");
    }
    assert!(readme.contains("Unofficial. Not affiliated with, endorsed by, or supported by hide.me."));
    assert!(readme.contains("ABSOLUTELY NO WARRANTY"));
}

/// The donation link appears exactly once in the whole documentation: in the last line of the README.
#[test]
fn donation_link_appears_once_at_the_end_of_the_readme() {
    let link = celata::donation::KOFI_URL;
    let domain = link
        .strip_prefix("https://")
        .and_then(|rest| rest.split('/').next())
        .expect("link has a host");
    let mut mentions = Vec::new();
    for file in DOCUMENTS {
        let content = read(file).to_lowercase();
        mentions.extend(std::iter::repeat_n(file, content.matches(domain).count()));
    }
    assert_eq!(
        mentions,
        ["README.md"],
        "the donation page may be linked only once, in README.md"
    );
    let readme = read("README.md");
    let last = readme.trim_end().lines().last().unwrap_or_default();
    assert!(
        last.contains(&format!("({link})")),
        "the last line of README.md must carry the link: {last}"
    );
    assert!(
        !root().join(".github/FUNDING.yml").exists(),
        "no FUNDING.yml: it would be a second place"
    );
}

/// Commands, names and numbers in the documentation are those of the program and the packages.
#[test]
fn documentation_matches_the_program() {
    use celata::state::{EXPECTED_VERSION, GROUP_COMMAND, RESTART_COMMAND, SERVICE_UNIT, START_COMMAND, STOP_COMMAND};
    let readme = read("README.md");
    for command in [START_COMMAND, RESTART_COMMAND, STOP_COMMAND, GROUP_COMMAND] {
        assert!(
            readme.contains(command),
            "README.md must show the command exactly as Celata does: {command}"
        );
    }
    let setup = read("docs/manual-setup.md");
    for command in [START_COMMAND, STOP_COMMAND, GROUP_COMMAND] {
        assert!(setup.contains(command), "docs/manual-setup.md: {command}");
    }
    assert!(readme.contains(SERVICE_UNIT) && read("SECURITY.md").contains(celata::daemon::DEFAULT_SOCKET));
    assert!(
        readme.contains(&format!("version {EXPECTED_VERSION}")),
        "README.md must name the tested client version"
    );
    assert!(read("docs/MAINTAINING.md").contains(&format!("version {EXPECTED_VERSION}")));

    let version = workspace_version();
    for package in [
        format!("celata_{version}-1_amd64.deb"),
        format!("celata-{version}-1.x86_64.rpm"),
    ] {
        assert!(
            readme.contains(&package),
            "README.md must name the package file {package}"
        );
    }
    assert!(readme.contains(&format!("Version {version}")));

    let rust =
        toml_string(&toml_table(&read("Cargo.toml"), "workspace.package"), "rust-version").expect("rust-version");
    for file in ["README.md", "docs/manual-setup.md"] {
        assert!(
            read(file).contains(&format!("Rust {rust} or newer")),
            "{file} must name Rust {rust} as the minimum"
        );
    }

    // The hint in the program quotes the heading of the documentation, and the documentation quotes the hint.
    let hint = celata::i18n::text(celata::i18n::Language::En, celata::i18n::Key::HintDnsLeak);
    assert!(hint.contains("“DNS leaks”"));
    let flat = read("docs/dns-leak.md").replace("\n> ", " ");
    assert!(
        flat.contains(hint),
        "docs/dns-leak.md must quote the hint word for word"
    );
}

/// The documentation is English: no German umlauts, and no reference to working notes.
#[test]
fn documentation_is_english() {
    for (file, content) in own_documents() {
        for (number, line) in content.lines().enumerate() {
            // The German translations in the metainfo are marked as such.
            if line.contains("xml:lang=\"de\"") {
                continue;
            }
            assert!(
                !line.chars().any(|character| "äöüÄÖÜß".contains(character)),
                "{file}:{}: German text in the English documentation: {line}",
                number + 1
            );
        }
    }
}

// --- Licence header and English comments ---

const COPYRIGHT_LINE: &str = "Copyright (C) 2026 derHouwer";

/// Packaging files whose format has no comments, or that carry their own licence statement.
const WITHOUT_HEADER: [&str; 4] = [
    "packaging/debian/changelog",
    "packaging/debian/copyright",
    "packaging/debian/source/format",
    "packaging/desktop/io.github.derhouwer.celata.metainfo.xml",
];

/// Files whose comments are German on purpose: the German texts, and a guard that stays in the working repository.
const GERMAN_ALLOWED: [&str; 2] = ["src/i18n/de.rs", "tests/name.rs"];

/// Words that give away a German comment. Deliberately coarse; together with the umlauts it catches whole sentences.
const GERMAN_WORDS: [&str; 10] = [
    "und", "oder", "nicht", "wird", "für", "eine", "wenn", "dass", "auch", "kein",
];

/// How a file marks its comments.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CommentStyle {
    /// `//` and nested `/* */`; strings, raw strings, characters and lifetimes are recognised.
    Rust,
    /// `//` and `/* */`; strings are recognised.
    Slint,
    /// `#` outside of strings, at the start of a word.
    Hash,
    /// `#` at the start of a line only.
    HashLine,
    /// `#` or `;` at the start of a line only.
    HashLineSemicolon,
}

/// Comment style of a file by its name, `None` for files that are compared as data (images, translations, XML).
fn comment_style(path: &str) -> Option<CommentStyle> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let extension = name.rsplit_once('.').map_or("", |(_, extension)| extension);
    match extension {
        "rs" => return Some(CommentStyle::Rust),
        "slint" => return Some(CommentStyle::Slint),
        "toml" | "sh" | "py" | "yml" | "yaml" | "install" | "prerm" | "postinst" | "postrm" | "preinst" => {
            return Some(CommentStyle::Hash);
        }
        "service" => return Some(CommentStyle::HashLineSemicolon),
        "desktop" | "conf" | "spec" | "docs" => return Some(CommentStyle::HashLine),
        _ => {}
    }
    if ["PKGBUILD", ".gitignore", ".gitattributes"].contains(&name) {
        return Some(CommentStyle::Hash);
    }
    let debian = ["control", "rules", "copyright", "changelog", "format"];
    (path.contains("/debian/") && debian.contains(&name)).then_some(CommentStyle::HashLine)
}

/// Index after the closing quote of the string or character that opens at `start`.
fn quoted_end(chars: &[char], start: usize, quote: char) -> usize {
    let mut index = start + 1;
    while index < chars.len() {
        match chars[index] {
            '\\' => index += 2,
            character if character == quote => return index + 1,
            _ => index += 1,
        }
    }
    chars.len()
}

/// Index after a raw string whose `#` or `"` is at `start`, `None` if no raw string starts there.
fn raw_end(chars: &[char], start: usize) -> Option<usize> {
    let hashes = chars[start..].iter().take_while(|character| **character == '#').count();
    let open = start + hashes;
    if chars.get(open) != Some(&'"') {
        return None;
    }
    let mut index = open + 1;
    while index < chars.len() {
        if chars[index] == '"' && chars[index + 1..].iter().take(hashes).filter(|c| **c == '#').count() == hashes {
            return Some(index + 1 + hashes);
        }
        index += 1;
    }
    Some(chars.len())
}

/// Comments of Rust or Slint source as (line, text).
fn c_like_comments(source: &str, rust: bool) -> Vec<(usize, String)> {
    let chars: Vec<char> = source.chars().collect();
    let at = |index: usize| chars.get(index).copied();
    let word_part = |index: usize| at(index).is_some_and(|character| character == '_' || character.is_alphanumeric());
    let newlines = |from: usize, to: usize| chars[from..to].iter().filter(|character| **character == '\n').count();
    let mut found = Vec::new();
    let (mut index, mut line) = (0, 1);
    while index < chars.len() {
        let character = chars[index];
        let end = match character {
            '/' if at(index + 1) == Some('/') => {
                let end = (index..chars.len())
                    .find(|&end| chars[end] == '\n')
                    .unwrap_or(chars.len());
                found.push((line, chars[index..end].iter().collect()));
                end
            }
            '/' if at(index + 1) == Some('*') => {
                let (mut depth, mut end) = (1, index + 2);
                while end < chars.len() && depth > 0 {
                    if chars[end] == '/' && at(end + 1) == Some('*') {
                        depth += 1;
                        end += 2;
                    } else if chars[end] == '*' && at(end + 1) == Some('/') {
                        depth -= 1;
                        end += 2;
                    } else {
                        end += 1;
                    }
                }
                found.push((line, chars[index..end].iter().collect()));
                end
            }
            '"' => quoted_end(&chars, index, '"'),
            // A character ('a', '\n') or a lifetime ('a, 'static).
            '\'' if rust && at(index + 1) == Some('\\') => quoted_end(&chars, index, '\''),
            '\'' if rust && at(index + 2) == Some('\'') => index + 3,
            _ if word_part(index) => {
                let mut end = index + 1;
                while word_part(end) {
                    end += 1;
                }
                let word: String = chars[index..end].iter().collect();
                let raw = rust && ["r", "br", "cr"].contains(&word.as_str());
                let prefixed = rust && ["b", "c"].contains(&word.as_str());
                match at(end) {
                    Some('#' | '"') if raw => raw_end(&chars, end).unwrap_or(end),
                    Some('"') if prefixed => quoted_end(&chars, end, '"'),
                    Some('\'') if rust && word == "b" => quoted_end(&chars, end, '\''),
                    _ => end,
                }
            }
            _ => index + 1,
        };
        line += newlines(index, end.min(chars.len()));
        index = end;
    }
    found
}

/// Comment of one line in a file with `#` comments, `None` if the line has none.
fn hash_comment(line: &str, style: CommentStyle) -> Option<&str> {
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') || (style == CommentStyle::HashLineSemicolon && trimmed.starts_with(';')) {
        return Some(trimmed);
    }
    if style != CommentStyle::Hash {
        return None;
    }
    let mut quote = None;
    let mut previous = ' ';
    let mut escaped = false;
    for (position, character) in line.char_indices() {
        if escaped {
            escaped = false;
        } else if let Some(open) = quote {
            if character == '\\' && open == '"' {
                escaped = true;
            } else if character == open {
                quote = None;
            }
        } else if character == '"' || character == '\'' {
            quote = Some(character);
        } else if character == '#' && previous.is_whitespace() {
            return Some(&line[position..]);
        }
        previous = character;
    }
    None
}

/// All comments of a file as (line, text).
fn comments(content: &str, style: CommentStyle) -> Vec<(usize, String)> {
    match style {
        CommentStyle::Rust => c_like_comments(content, true),
        CommentStyle::Slint => c_like_comments(content, false),
        _ => content
            .lines()
            .enumerate()
            .filter_map(|(number, line)| Some((number + 1, hash_comment(line, style)?.to_owned())))
            .collect(),
    }
}

/// The umlaut or word that makes a comment look German, `None` for an English comment.
fn german_part(comment: &str) -> Option<String> {
    if let Some(umlaut) = comment.chars().find(|character| "äöüÄÖÜß".contains(*character)) {
        return Some(umlaut.to_string());
    }
    comment
        .split(|character: char| character != '_' && !character.is_alphanumeric())
        .find(|word| GERMAN_WORDS.contains(&word.to_lowercase().as_str()))
        .map(str::to_owned)
}

/// All files below `relative`, as sorted paths relative to the project directory; build output is skipped.
fn files_below(relative: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut open = vec![relative.to_owned()];
    while let Some(directory) = open.pop() {
        let entries = fs::read_dir(root().join(&directory))
            .unwrap_or_else(|error| panic!("{directory} is not readable: {error}"));
        for entry in entries {
            let entry = entry.expect("directory entry is readable");
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = format!("{directory}/{name}");
            if entry.path().is_dir() {
                if name != "target" && name != "vendor" {
                    open.push(path);
                }
            } else {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/// Hand-written files that are published: program, tests, user interface, packaging and build configuration.
fn published_sources() -> Vec<String> {
    let mut files: Vec<String> = ["src", "tests", "crates", "packaging"]
        .into_iter()
        .flat_map(files_below)
        .collect();
    files.extend(
        files_below("ui")
            .into_iter()
            .filter(|path| comment_style(path) == Some(CommentStyle::Slint)),
    );
    // A package build replaces the directory `.cargo` with its own; the file is then not there to be read.
    let configuration = ["Cargo.toml", "deny.toml", ".cargo/audit.toml"];
    files.extend(
        configuration
            .into_iter()
            .filter(|path| root().join(path).is_file())
            .map(str::to_owned),
    );
    files
}

/// The reader of comments must not take a string for a comment, or a comment for code.
#[test]
fn comment_reader_tells_comments_from_strings() {
    let rust = "let a = \"sdns://x\"; // one\nlet b = r#\"// not \"one\" \"#; /* two\n /* inner */ */ let c = '\"'; // three\n";
    let texts: Vec<(usize, String)> = c_like_comments(rust, true);
    assert_eq!(
        texts,
        [
            (1, "// one".to_owned()),
            (2, "/* two\n /* inner */ */".to_owned()),
            (3, "// three".to_owned())
        ]
    );
    assert_eq!(
        c_like_comments("fn f<'a>(x: &'a str) {} // lifetime", true)[0].1,
        "// lifetime"
    );
    assert_eq!(c_like_comments("text: \"a // b\"; // slint", false)[0].1, "// slint");
    assert_eq!(
        hash_comment("key = \"a # b\" # note", CommentStyle::Hash),
        Some("# note")
    );
    assert_eq!(hash_comment("url = a#b", CommentStyle::Hash), None);
    assert_eq!(hash_comment("Exec=celata # no comment", CommentStyle::HashLine), None);
    assert_eq!(hash_comment("; unit", CommentStyle::HashLineSemicolon), Some("; unit"));
    assert_eq!(german_part("// Prüft den Wert"), Some("ü".to_owned()));
    assert_eq!(german_part("// Liest die Datei und Ordner"), Some("und".to_owned()));
    assert_eq!(
        german_part("// see `ohne_sperre_wird_nicht_verbunden`, a fundamental check"),
        None
    );
}

/// Every hand-written source file states its licence in the first lines.
#[test]
fn source_files_start_with_the_license_header() {
    let mut checked = 0;
    for path in published_sources() {
        let in_packaging = path.starts_with("packaging/");
        let is_code = matches!(comment_style(&path), Some(CommentStyle::Rust | CommentStyle::Slint));
        let is_source = is_code || in_packaging;
        if !is_source || WITHOUT_HEADER.contains(&path.as_str()) || path == "tests/name.rs" {
            continue;
        }
        let marker = match comment_style(&path) {
            Some(CommentStyle::Rust | CommentStyle::Slint) => "//",
            Some(_) => "#",
            None => panic!("{path}: unknown kind of file; give it a comment style or list it in WITHOUT_HEADER"),
        };
        let content = read(&path);
        let lines: Vec<&str> = content
            .lines()
            .skip_while(|line| line.starts_with("#!"))
            .take(2)
            .collect();
        assert_eq!(
            lines,
            [
                format!("{marker} SPDX-License-Identifier: {LICENSE_ID}"),
                format!("{marker} {COPYRIGHT_LINE}")
            ],
            "{path} does not start with the licence header"
        );
        checked += 1;
    }
    assert!(checked >= 50, "only {checked} files were checked");
    for path in WITHOUT_HEADER {
        assert!(
            root().join(path).is_file(),
            "{path} is listed in WITHOUT_HEADER but missing"
        );
    }
}

/// Comments are English everywhere, so that everybody can read and review the code.
#[test]
fn comments_are_english() {
    let mut read_comments = 0;
    let mut german = Vec::new();
    for path in published_sources() {
        let Some(style) = comment_style(&path) else {
            continue;
        };
        if GERMAN_ALLOWED.contains(&path.as_str()) {
            continue;
        }
        for (line, comment) in comments(&read(&path), style) {
            read_comments += 1;
            if let Some(part) = german_part(&comment) {
                german.push(format!("{path}:{line}: “{part}” in {comment}"));
            }
        }
    }
    assert!(german.is_empty(), "German comments:\n{}", german.join("\n"));
    assert!(read_comments >= 1500, "only {read_comments} comments were read");
}
