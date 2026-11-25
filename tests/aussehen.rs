// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! Checks the source code for the promises of the two appearances (light and dark): `Palette.color-scheme` is the
//! only place that holds the appearance and is set only by `set-appearance`; the button in the header has its
//! name, its icons and only colours from `Theme`; the icon in the system tray does not follow the appearance;
//! flags and the logo of "Best location" get their border in the dark appearance; Rust sets the stored
//! appearance before the first picture and the button only saves. The colours themselves are checked by
//! `tests/kontrast.rs`.
#![forbid(unsafe_code)]

use std::fs;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(path: &str) -> String {
    fs::read_to_string(root().join(path)).unwrap_or_else(|error| panic!("{path} nicht lesbar: {error}"))
}

/// The code of a Slint line without its comment.
fn code(line: &str) -> &str {
    line.split("//").next().unwrap_or_default()
}

/// Text of a component or function: from the line that contains `start` to the first following line that is
/// exactly `}` at the indentation of the start line.
fn block<'a>(content: &'a str, start: &str) -> &'a str {
    let begin = content.find(start).unwrap_or_else(|| panic!("{start:?} fehlt"));
    let line_start = content[..begin].rfind('\n').map_or(0, |i| i + 1);
    let indent = content[line_start..].len() - content[line_start..].trim_start().len();
    let closing = format!("\n{}}}\n", " ".repeat(indent));
    let end = content[begin..]
        .find(&closing)
        .unwrap_or_else(|| panic!("Ende von {start:?} nicht gefunden"));
    &content[begin..begin + end + closing.len()]
}

/// Does the line assign `Palette.color-scheme` (and not only read or compare it)?
fn assigns_palette(line: &str) -> bool {
    let code = code(line);
    code.match_indices("Palette.color-scheme").any(|(i, token)| {
        let rest = code[i + token.len()..].trim_start();
        rest.starts_with('=') && !rest.starts_with("==")
    })
}

fn slint_files() -> Vec<(String, String)> {
    let mut files: Vec<(String, String)> = fs::read_dir(root().join("ui"))
        .expect("ui/ lesbar")
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "slint"))
        .map(|path| {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let content = fs::read_to_string(&path).unwrap();
            (name, content)
        })
        .collect();
    files.sort();
    files
}

/// The appearance has exactly one source: `Palette.color-scheme` is assigned only in `ui/main.slint` and there only
/// inside `set-appearance`.
#[test]
fn das_aussehen_hat_genau_eine_quelle() {
    let mut zuweisungen = Vec::new();
    for (name, content) in slint_files() {
        for (number, line) in content.lines().enumerate() {
            if assigns_palette(line) {
                zuweisungen.push((name.clone(), number + 1));
            }
        }
    }
    assert!(
        zuweisungen.iter().all(|(name, _)| name == "main.slint"),
        "Palette.color-scheme wird außerhalb von ui/main.slint gesetzt: {zuweisungen:?}"
    );
    assert_eq!(zuweisungen.len(), 1, "genau eine Zuweisung erwartet: {zuweisungen:?}");
    let main = read("ui/main.slint");
    let function = block(&main, "    public function set-appearance(dark: bool) {");
    assert!(
        function.lines().any(assigns_palette),
        "die Zuweisung steht nicht in set-appearance"
    );
    assert!(main.contains("callback appearance-toggled(bool);"));
    // The derived value is never set.
    let theme = read("ui/theme.slint");
    assert!(theme.contains("out property <bool> is-dark: Palette.color-scheme == ColorScheme.dark;"));
}

#[test]
fn der_knopf_hat_name_symbole_und_nur_theme_farben() {
    let widgets = read("ui/widgets.slint");
    let button = block(&widgets, "export component AppearanceButton inherits Rectangle {");
    for expected in [
        "accessible-role: button;",
        "accessible-action-default",
        "@tr(\"Light appearance\")",
        "@tr(\"Dark appearance\")",
        "icons/moon.svg",
        "icons/sun.svg",
        "Key.Return",
        "FocusScope",
    ] {
        assert!(button.contains(expected), "{expected:?} fehlt im Knopf");
    }
    for line in button.lines().map(code) {
        let property = line.trim_start().split(':').next().unwrap_or_default();
        if matches!(property, "background" | "border-color" | "colorize" | "color") {
            assert!(
                line.contains("Theme.") || line.contains("transparent"),
                "Farbe nicht aus Theme: {line:?}"
            );
        }
    }
    // In the header, as the last element of the row.
    let main = read("ui/main.slint");
    assert!(main.contains("AppearanceButton {"));
    assert!(main.contains("root.set-appearance(!Theme.is-dark);"));
    assert!(main.contains("root.appearance-toggled(Theme.is-dark);"));
}

#[test]
fn die_symbole_tragen_den_lucide_kopf() {
    for name in ["moon", "sun"] {
        let svg = read(&format!("ui/icons/{name}.svg"));
        assert!(
            svg.starts_with("<!-- @license lucide-static v1.70.0 - ISC -->"),
            "ui/icons/{name}.svg ohne Lucide-Kopf"
        );
        assert!(
            svg.contains(&format!("lucide-{name}")),
            "ui/icons/{name}.svg ist nicht das Symbol {name}"
        );
    }
}

/// The icon in the system tray, its menu and its colours do not follow the appearance of the window.
#[test]
fn das_symbol_im_systemabschnitt_folgt_dem_aussehen_nicht() {
    let tray = read("ui/tray.slint");
    let program: String = tray.lines().map(code).collect::<Vec<_>>().join("\n");
    for word in ["Theme", "Palette", "is-dark", "set-appearance"] {
        assert!(!program.contains(word), "ui/tray.slint enthält {word}");
    }
}

/// In the dark appearance flags and the logo of "Best location" get a border of 1 px `image-edge`; in the light
/// appearance there is none.
#[test]
fn flaggen_und_logo_haben_im_dunklen_aussehen_einen_rand() {
    let widgets = read("ui/widgets.slint");
    let flag = block(&widgets, "export component Flag inherits Rectangle {");
    let best = block(&widgets, "export component BestIcon inherits Rectangle {");
    for (name, text) in [("Flag", flag), ("BestIcon", best)] {
        assert!(text.contains("Theme.image-edge"), "{name} nutzt image-edge nicht");
        assert!(
            text.contains("Theme.is-dark"),
            "{name} macht den Rand nicht vom Aussehen abhängig"
        );
        assert!(text.contains("1px"), "{name}: Rand nicht 1 px");
    }
    // The logo itself does not change: its file has no `Theme`.
    assert!(!read("ui/icons/celata.svg").contains("Theme"));
}

/// Rust sets the stored appearance before the first picture, also for the hidden start, and the button only saves.
#[test]
fn rust_setzt_das_aussehen_vor_dem_ersten_bild() {
    let main = read("src/main.rs");
    let call = main
        .find("connect_appearance(&window")
        .expect("connect_appearance wird im Start aufgerufen");
    let first_show = main
        .find("StartMode::Window => window.show()")
        .expect("Start mit Fenster");
    let tray_only = main.find("StartMode::TrayOnly { wait }").expect("versteckter Start");
    assert!(
        call < first_show && call < tray_only,
        "das Aussehen muss vor dem Start gesetzt werden"
    );
    let function = block(&main, "fn connect_appearance(");
    assert!(function.contains("invoke_set_appearance("));
    assert!(function.contains("on_appearance_toggled"));
    assert!(function.contains("save_app("));
    // Saving only: no service, no network, no reconnect.
    for forbidden in [
        "monitor",
        "service",
        "client",
        "Client",
        "net::",
        "start_connect",
        "push(",
    ] {
        assert!(
            !function.contains(forbidden),
            "connect_appearance enthält {forbidden:?}"
        );
    }
    // Rust never reads the appearance back from the window.
    assert!(!main.contains("get_appearance"));
}
