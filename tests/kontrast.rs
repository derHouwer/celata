// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! Checks the contrasts of the colours from `ui/theme.slint` according to WCAG 2.1.
//!
//! Every pair of text and background that the user interface uses is in [`PAARE`] and must reach at
//! least 4.5 : 1.
#![forbid(unsafe_code)]

use std::collections::HashMap;

const THEME: &str = include_str!("../ui/theme.slint");
const MINDESTWERT: f64 = 4.5;

/// (text, background)
const PAARE: &[(&str, &str)] = &[
    ("on-dark", "sidebar"),
    ("on-dark", "sidebar-selected"),
    ("on-dark", "sidebar-hover"),
    ("on-dark", "topbar"),
    ("on-dark", "accent"),
    ("on-dark", "power-field"),
    ("on-dark", "tone-alert"),
    ("text", "surface"),
    ("text", "card"),
    ("text", "card-warning"),
    ("text", "row-selected"),
    ("text", "row-hover"),
    ("text-soft", "card"),
    ("text-soft", "surface"),
    ("text-soft", "card-warning"),
    ("text-soft", "row-selected"),
    ("text-soft", "row-hover"),
    ("accent", "card"),
    ("tab-idle", "card"),
    ("tone-alert", "card"),
    ("tone-alert", "card-warning"),
    ("disabled-text", "disabled-bg"),
    ("free-text", "card"),
    ("free-text", "row-selected"),
    ("free-text", "row-hover"),
    ("version-text", "surface"),
    // Page "Settings"
    ("version-text", "card"),
    ("tone-alert", "surface"),
    ("disabled-text", "card"),
    ("command-fg", "command-bg"),
    // Red main button
    ("on-dark", "alert-pressed"),
    // Donation hint
    ("topbar", "card"),
    ("topbar", "row-hover"),
    ("tone-alert", "row-hover"),
    // White button and location card while the mouse is over them. `accent` on this tint reaches only 4.18,
    // so text and border turn to `button-hover-text` there.
    ("button-hover-text", "button-hover"),
    ("tone-alert", "button-hover"),
    ("text", "button-hover"),
    ("text-soft", "button-hover"),
];

/// Reads all lines `out property <color> name: #rrggbb;`.
fn farben() -> HashMap<String, [u8; 3]> {
    THEME
        .lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("out property <color> ")?;
            let (name, value) = rest.split_once(':')?;
            let hex = value.trim().strip_prefix('#')?.strip_suffix(';')?;
            if hex.len() != 6 {
                return None;
            }
            let channel = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
            Some((name.trim().to_owned(), [channel(0)?, channel(2)?, channel(4)?]))
        })
        .collect()
}

fn leuchtdichte(rgb: [u8; 3]) -> f64 {
    let linear = |c: u8| {
        let c = f64::from(c) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(rgb[0]) + 0.7152 * linear(rgb[1]) + 0.0722 * linear(rgb[2])
}

fn kontrast(a: [u8; 3], b: [u8; 3]) -> f64 {
    let (la, lb) = (leuchtdichte(a), leuchtdichte(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

#[test]
fn formel_stimmt_mit_bekannten_werten() {
    assert!((kontrast([0, 0, 0], [255, 255, 255]) - 21.0).abs() < 0.01);
    // White on #127bb0: 4.68, calculated independently
    assert!((kontrast([255, 255, 255], [0x12, 0x7b, 0xb0]) - 4.68).abs() < 0.01);
}

#[test]
fn jede_genannte_farbe_steht_im_farbschema() {
    let farben = farben();
    for (schrift, hintergrund) in PAARE {
        assert!(farben.contains_key(*schrift), "{schrift} fehlt in ui/theme.slint");
        assert!(
            farben.contains_key(*hintergrund),
            "{hintergrund} fehlt in ui/theme.slint"
        );
    }
}

#[test]
fn alle_farbpaare_sind_gut_lesbar() {
    let farben = farben();
    let schwach: Vec<String> = PAARE
        .iter()
        .filter_map(|(schrift, hintergrund)| {
            let wert = kontrast(*farben.get(*schrift)?, *farben.get(*hintergrund)?);
            (wert < MINDESTWERT).then(|| format!("{schrift} auf {hintergrund}: {wert:.2}"))
        })
        .collect();
    assert!(schwach.is_empty(), "Kontrast unter {MINDESTWERT}: {schwach:?}");
}

/// The blue tones that have a prepared replacement `alt-<name>` (second accent colour).
const ACCENT_TONES: [&str; 11] = [
    "topbar",
    "sidebar",
    "sidebar-selected",
    "sidebar-hover",
    "accent",
    "power-field",
    "focus",
    "button-hover",
    "button-hover-text",
    "row-hover",
    "row-selected",
];

/// The prepared second accent colour passes the same pairs, so that switching needs no further check.
#[test]
fn second_accent_colour_is_readable_too() {
    let farben = farben();
    let variante = |name: &str| -> [u8; 3] {
        if ACCENT_TONES.contains(&name) {
            farben[&format!("alt-{name}")]
        } else {
            farben[name]
        }
    };
    for name in ACCENT_TONES {
        assert!(
            farben.contains_key(&format!("alt-{name}")),
            "alt-{name} fehlt in ui/theme.slint"
        );
        assert_ne!(
            farben[&format!("alt-{name}")],
            farben[name],
            "alt-{name} ist keine andere Farbe"
        );
    }
    let alt_count = farben.keys().filter(|name| name.starts_with("alt-")).count();
    assert_eq!(alt_count, ACCENT_TONES.len(), "eine alt-Farbe ohne Gegenstück");
    let schwach: Vec<String> = PAARE
        .iter()
        .filter_map(|(schrift, hintergrund)| {
            let wert = kontrast(variante(schrift), variante(hintergrund));
            (wert < MINDESTWERT).then(|| format!("{schrift} auf {hintergrund}: {wert:.2}"))
        })
        .collect();
    assert!(
        schwach.is_empty(),
        "zweite Akzentfarbe, Kontrast unter {MINDESTWERT}: {schwach:?}"
    );
}

/// Colours live only in `ui/theme.slint`. No other `.slint` file contains a colour value.
#[test]
fn farbwerte_stehen_nur_im_farbschema() {
    let ui = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("ui");
    let mut found = Vec::new();
    let mut checked = 0;
    for entry in std::fs::read_dir(&ui).expect("ui/ lesbar") {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if path.extension().is_none_or(|extension| extension != "slint") || name == "theme.slint" {
            continue;
        }
        checked += 1;
        for (number, line) in std::fs::read_to_string(&path).unwrap().lines().enumerate() {
            let code = line.split("//").next().unwrap_or_default();
            let literal = code.split('#').skip(1).any(|rest| {
                let digits = rest.bytes().take_while(u8::is_ascii_hexdigit).count();
                matches!(digits, 3 | 4 | 6 | 8)
            });
            if literal {
                found.push(format!("ui/{name}:{}", number + 1));
            }
        }
    }
    assert!(checked >= 5, "zu wenige .slint-Dateien gefunden");
    assert!(found.is_empty(), "Farbwerte außerhalb von ui/theme.slint: {found:?}");
}
