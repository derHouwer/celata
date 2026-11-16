// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! Checks the translations of the user interface: every text in `@tr("…")` from `ui/*.slint` has a
//! non-empty translation with the same placeholders in every language below `ui/translations/`, and
//! there are no orphaned entries. The texts from Rust are checked by the compiler (`src/i18n`); their
//! placeholders are checked by a module test there.
//!
//! Without external crates: small readers for `@tr` and the `.po` format, like `tests/kontrast.rs` reads
//! the colour scheme.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// All `.slint` files in `ui/`, sorted, with their content.
fn slint_files() -> Vec<(String, String)> {
    let mut files: Vec<PathBuf> = fs::read_dir(root().join("ui"))
        .expect("ui/ lesbar")
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "slint"))
        .collect();
    files.sort();
    files
        .into_iter()
        .map(|path| {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            (name, fs::read_to_string(&path).expect("Datei lesbar"))
        })
        .collect()
}

/// Reads a string starting at `text[0] == '"'`. Returns the value and the length in the source text.
fn read_string(text: &str) -> Option<(String, usize)> {
    let mut chars = text.char_indices();
    if chars.next()?.1 != '"' {
        return None;
    }
    let mut value = String::new();
    while let Some((index, c)) = chars.next() {
        match c {
            '"' => return Some((value, index + 1)),
            '\\' => {
                let (_, escaped) = chars.next()?;
                match escaped {
                    'n' => value.push('\n'),
                    't' => value.push('\t'),
                    '"' | '\\' => value.push(escaped),
                    other => {
                        value.push('\\');
                        value.push(other);
                    }
                }
            }
            other => value.push(other),
        }
    }
    None
}

/// Part of a line before a line comment; `//` in strings (such as `sdns://`) does not count.
fn code_part(line: &str) -> &str {
    let mut in_string = false;
    let mut escaped = false;
    for (index, c) in line.char_indices() {
        match c {
            _ if escaped => escaped = false,
            '\\' if in_string => escaped = true,
            '"' => in_string = !in_string,
            '/' if !in_string && line[index + 1..].starts_with('/') => return &line[..index],
            _ => {}
        }
    }
    line
}

/// A `@tr` in a `.slint` file.
#[derive(Debug)]
struct Use {
    context: String,
    id: String,
    place: String,
}

/// All `@tr("…")` and `@tr("context" => "…")` outside of line comments.
fn tr_uses() -> Vec<Use> {
    let mut found = Vec::new();
    for (name, content) in slint_files() {
        for (number, line) in content.lines().enumerate() {
            let code = code_part(line);
            let mut rest = code;
            while let Some(start) = rest.find("@tr(") {
                let after = rest[start + 4..].trim_start();
                let place = format!("ui/{name}:{}", number + 1);
                let (first, length) = read_string(after).unwrap_or_else(|| panic!("{place}: @tr ohne Zeichenkette"));
                let tail = after[length..].trim_start();
                let use_ = match tail.strip_prefix("=>") {
                    Some(tail) => {
                        let (id, _) = read_string(tail.trim_start())
                            .unwrap_or_else(|| panic!("{place}: @tr mit Kontext ohne Text"));
                        Use {
                            context: first,
                            id,
                            place,
                        }
                    }
                    None => Use {
                        context: String::new(),
                        id: first,
                        place,
                    },
                };
                found.push(use_);
                rest = &rest[start + 4..];
            }
        }
    }
    found
}

/// An entry of a `.po` file: (context, msgid) → msgstr.
type Catalog = BTreeMap<(String, String), String>;

/// Reads a `.po` file: `msgctxt`, `msgid`, `msgstr` with continuation lines; comments and header are
/// skipped. Plural forms are not allowed.
fn read_po(path: &Path) -> Catalog {
    let content = fs::read_to_string(path).unwrap_or_else(|_| panic!("{} lesbar", path.display()));
    let mut catalog = Catalog::new();
    let mut fields: BTreeMap<&str, String> = BTreeMap::new();
    let mut current: Option<&str> = None;
    let mut finish = |fields: &mut BTreeMap<&str, String>| {
        if let Some(id) = fields.remove("msgid") {
            let msgctxt = fields.remove("msgctxt").unwrap_or_default();
            let text = fields
                .remove("msgstr")
                .unwrap_or_else(|| panic!("{}: msgstr fehlt zu {id:?}", path.display()));
            if !id.is_empty() {
                assert!(
                    catalog.insert((msgctxt, id.clone()), text).is_none(),
                    "{}: {id:?} steht doppelt",
                    path.display()
                );
            }
        }
        fields.clear();
    };
    for line in content.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('"') {
            let key = current.unwrap_or_else(|| panic!("{}: Folgezeile ohne Feld: {line}", path.display()));
            let (value, _) = read_string(line).unwrap_or_else(|| panic!("{}: unlesbar: {line}", path.display()));
            fields.get_mut(key).unwrap().push_str(&value);
            continue;
        }
        let (key, value) = line
            .split_once(' ')
            .unwrap_or_else(|| panic!("{}: unlesbar: {line}", path.display()));
        let key = match key {
            "msgctxt" => "msgctxt",
            "msgid" => "msgid",
            "msgstr" => "msgstr",
            other => panic!(
                "{}: Feld {other} ist nicht erlaubt (keine Mehrzahlformen)",
                path.display()
            ),
        };
        if key != "msgstr" && fields.contains_key("msgstr") {
            finish(&mut fields);
        }
        let (value, _) = read_string(value.trim()).unwrap_or_else(|| panic!("{}: unlesbar: {line}", path.display()));
        fields.insert(key, value);
        current = Some(key);
    }
    finish(&mut fields);
    catalog
}

/// Translations per language: directory name below `ui/translations/` → content of `celata-ui.po`.
fn catalogs() -> Vec<(String, Catalog)> {
    let dir = root().join("ui/translations");
    let mut languages: Vec<String> = fs::read_dir(&dir)
        .expect("ui/translations lesbar")
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    languages.sort();
    languages
        .into_iter()
        .map(|language| {
            let path = dir.join(&language).join("LC_MESSAGES/celata-ui.po");
            assert!(path.is_file(), "{} fehlt", path.display());
            let catalog = read_po(&path);
            (language, catalog)
        })
        .collect()
}

/// Placeholders of a text: number of `{}` and set of `{n}`; `{{` and `}}` are none.
fn placeholders(text: &str) -> (usize, BTreeSet<String>) {
    let text = text.replace("{{", "").replace("}}", "");
    let mut plain = 0;
    let mut numbered = BTreeSet::new();
    let mut rest = text.as_str();
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        match after.find('}') {
            Some(0) => plain += 1,
            Some(end) if after[..end].bytes().all(|b| b.is_ascii_digit()) => {
                numbered.insert(after[..end].to_owned());
            }
            _ => {}
        }
        rest = after;
    }
    (plain, numbered)
}

#[test]
fn leser_verstehen_die_formate() {
    assert_eq!(
        read_string(r#""a \"b\" \\ c" rest"#),
        Some((r#"a "b" \ c"#.to_owned(), 14))
    );
    assert_eq!(
        placeholders("{} und {0} und {{x}}"),
        (1, BTreeSet::from(["0".to_owned()]))
    );
    assert_eq!(
        code_part(r#"label: @tr("a (sdns://…)"); // Kommentar"#),
        r#"label: @tr("a (sdns://…)"); "#
    );
    assert_eq!(code_part(r#"text: "a\"//b"; // c"#), r#"text: "a\"//b"; "#);
    let dir = std::env::temp_dir().join(format!("celata-sprachen-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("probe.po");
    fs::write(
        &path,
        "# Kommentar\nmsgid \"\"\nmsgstr \"\"\n\"Language: de\\n\"\n\nmsgid \"Hello\"\nmsgstr \"Hallo\"\n\n\
         msgctxt \"Knopf\"\nmsgid \"Open\"\nmsgstr \"\"\n\"Öff\"\n\"nen\"\n",
    )
    .unwrap();
    let catalog = read_po(&path);
    assert_eq!(catalog.len(), 2);
    assert_eq!(catalog[&(String::new(), "Hello".to_owned())], "Hallo");
    assert_eq!(catalog[&("Knopf".to_owned(), "Open".to_owned())], "Öffnen");
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn jede_uebersetzung_ist_vollstaendig() {
    let uses = tr_uses();
    let mut missing = Vec::new();
    for (language, catalog) in catalogs() {
        for use_ in &uses {
            match catalog.get(&(use_.context.clone(), use_.id.clone())) {
                Some(text) if !text.trim().is_empty() => {}
                Some(_) => missing.push(format!("{language}: leer: {:?} ({})", use_.id, use_.place)),
                None => missing.push(format!("{language}: fehlt: {:?} ({})", use_.id, use_.place)),
            }
        }
    }
    assert!(missing.is_empty(), "Übersetzungen fehlen:\n{}", missing.join("\n"));
}

#[test]
fn keine_verwaisten_uebersetzungen() {
    let used: BTreeSet<(String, String)> = tr_uses().into_iter().map(|use_| (use_.context, use_.id)).collect();
    let mut orphans = Vec::new();
    for (language, catalog) in catalogs() {
        for key in catalog.keys().filter(|key| !used.contains(*key)) {
            orphans.push(format!("{language}: {:?} (Kontext {:?})", key.1, key.0));
        }
    }
    assert!(
        orphans.is_empty(),
        "Einträge ohne @tr in ui/*.slint:\n{}",
        orphans.join("\n")
    );
}

#[test]
fn platzhalter_sind_in_jeder_sprache_gleich() {
    let mut wrong = Vec::new();
    for (language, catalog) in catalogs() {
        for ((_, id), text) in &catalog {
            if placeholders(id) != placeholders(text) {
                wrong.push(format!("{language}: {id:?} → {text:?}"));
            }
        }
    }
    assert!(wrong.is_empty(), "Platzhalter weichen ab:\n{}", wrong.join("\n"));
}

#[test]
fn deutsch_ist_vorhanden() {
    assert!(
        catalogs().iter().any(|(language, _)| language == "de"),
        "ui/translations/de fehlt"
    );
}

// --- Source code checks ---

/// Strings of a line of code with the text immediately before each.
fn literals(code: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let mut rest = code;
    let mut offset = 0;
    while let Some(start) = rest.find('"') {
        let Some((value, length)) = read_string(&rest[start..]) else {
            break;
        };
        found.push((code[..offset + start].to_owned(), value));
        offset += start + length;
        rest = &rest[start + length..];
    }
    found
}

/// Texts in `.slint` that may stand without `@tr`, each with a reason.
const SLINT_EXCEPTIONS: [(&str, &str); 8] = [
    ("Celata", "Name des Programms"),
    (
        "Sprache · Language",
        "Überschrift der Sprachauswahl, in jeder Sprache gleich",
    ),
    ("IPv4", "Fachbegriff, in jeder Sprache gleich"),
    ("IPv6", "Fachbegriff, in jeder Sprache gleich"),
    ("DNS", "Fachbegriff, in jeder Sprache gleich"),
    ("9.9.9.9:53", "Beispielwert"),
    ("192.168.1.0/24", "Beispielwert"),
    ("sdns://…", "Beispielwert"),
];

#[test]
fn keine_unmarkierten_texte_in_slint() {
    let mut found = Vec::new();
    for (name, content) in slint_files() {
        for (number, line) in content.lines().enumerate() {
            let code = code_part(line);
            if code.trim_start().starts_with("import") || code.contains("} from ") {
                continue;
            }
            for (before, value) in literals(code) {
                let marked = before.trim_end().ends_with("@tr(") || before.trim_end().ends_with("=>");
                let image = before.trim_end().ends_with("@image-url(");
                let text_like = value.chars().any(char::is_alphabetic)
                    && (value.contains(' ') || value.chars().any(|c| c.is_uppercase() || !c.is_ascii()));
                if !marked && !image && text_like && !SLINT_EXCEPTIONS.iter().any(|(allowed, _)| *allowed == value) {
                    found.push(format!("ui/{name}:{}: {value:?}", number + 1));
                }
            }
        }
    }
    assert!(found.is_empty(), "Texte ohne @tr:\n{}", found.join("\n"));
}

/// All `.rs` files below `src/`, sorted.
fn rust_sources(dir: &Path, found: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .expect("Ordner lesbar")
        .map(|entry| entry.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            rust_sources(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            found.push(path);
        }
    }
}

/// Typical German words; together with umlauts and the sharp s a deliberately coarse check.
const GERMAN_HINTS: [&str; 10] = [
    " und ", " der ", " die ", " das ", " nicht ", " wird ", " ist ", " dein", " du ", " mit ",
];

/// Strings in Rust that may look German, with a reason.
const RUST_EXCEPTIONS: [(&str, &str, &str); 1] = [(
    "src/donation.rs",
    "dein-name",
    "Platzhalter, den die Prüfung des Spendenlinks ablehnt; nie angezeigt",
)];

#[test]
fn kein_deutsch_im_rust_code() {
    let mut files = Vec::new();
    rust_sources(&root().join("src"), &mut files);
    let mut found = Vec::new();
    for path in files {
        let relative = path.strip_prefix(root()).unwrap().to_string_lossy().into_owned();
        // The German texts are only here.
        if relative == "src/i18n/de.rs" {
            continue;
        }
        let content = fs::read_to_string(&path).expect("Datei lesbar");
        for (number, line) in content.lines().enumerate() {
            if line.trim_start().starts_with("#[cfg(test)]") {
                break;
            }
            for (_, value) in literals(code_part(line)) {
                let padded = format!(" {} ", value.to_lowercase());
                let german = value.chars().any(|c| "äöüÄÖÜß".contains(c))
                    || GERMAN_HINTS.iter().any(|word| padded.contains(word));
                let allowed = RUST_EXCEPTIONS
                    .iter()
                    .any(|(file, text, _)| *file == relative && *text == value);
                if german && !allowed {
                    found.push(format!("{relative}:{}: {value:?}", number + 1));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "Deutscher Text außerhalb von src/i18n/de.rs:\n{}",
        found.join("\n")
    );
}

#[test]
fn kuerzen_nur_an_freigegebenen_stellen() {
    let mut found = Vec::new();
    for (name, content) in slint_files() {
        for (number, line) in content.lines().enumerate() {
            if code_part(line).contains("overflow: elide") && !line.contains("// may be cut:") {
                found.push(format!("ui/{name}:{}", number + 1));
            }
        }
    }
    assert!(
        found.is_empty(),
        "`overflow: elide` ohne den Vermerk `// may be cut:` mit Grund:\n{}",
        found.join("\n")
    );
}

/// Narrow places of the user interface with a maximum length in every language.
const LIMITS: [(&str, usize); 8] = [
    ("Overview", 16),
    ("Locations", 16),
    ("Settings", 16),
    ("Standard", 14),
    ("Streaming", 14),
    ("Disconnect and close", 32),
    ("Cancel", 32),
    ("Unblock internet", 32),
];

#[test]
fn laengengrenzen_enger_stellen() {
    let used: BTreeSet<String> = tr_uses().into_iter().map(|use_| use_.id).collect();
    let mut found = Vec::new();
    for (id, limit) in LIMITS {
        assert!(
            used.contains(id),
            "{id:?} wird in ui/*.slint nicht mehr genutzt; Liste anpassen"
        );
        if id.chars().count() > limit {
            found.push(format!("en: {id:?} länger als {limit}"));
        }
        for (language, catalog) in catalogs() {
            if let Some(text) = catalog.get(&(String::new(), id.to_owned()))
                && text.chars().count() > limit
            {
                found.push(format!("{language}: {text:?} länger als {limit}"));
            }
        }
    }
    assert!(
        found.is_empty(),
        "Zu lange Texte an engen Stellen:\n{}",
        found.join("\n")
    );
}
