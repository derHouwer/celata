// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! Checks the colours from `ui/theme.slint` for both appearances (light and dark) according to WCAG 2.1.
//!
//! Every colour is one line `out property <color> name: pick(#light, #dark);` (only the second accent colour
//! `alt-…` has a single value). The light values stay as they were ([`LIGHT_TODAY`]). Every pair of text and
//! background that the user interface uses is in [`PAARE`] and must reach at least 4.5 : 1 in both appearances.
//! Controls without text (switch, star, focus frame, state dot) are in [`BEDIENELEMENTE`] and must reach at
//! least 3 : 1. Pairs that only exist in one appearance are in [`NUR_HELL`] and [`NUR_DUNKEL`]; pairs that
//! deliberately stay below the limit are in [`AUSNAHMEN`].
#![forbid(unsafe_code)]

use std::collections::{HashMap, HashSet};

const THEME: &str = include_str!("../ui/theme.slint");
const MINDESTWERT: f64 = 4.5;
/// WCAG 1.4.11 (non-text contrast): controls without text, such as the outline of the star.
const MINDESTWERT_BEDIENELEMENT: f64 = 3.0;

/// The two appearances.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Aussehen {
    Hell,
    Dunkel,
}

impl Aussehen {
    const BEIDE: [Aussehen; 2] = [Aussehen::Hell, Aussehen::Dunkel];

    fn name(self) -> &'static str {
        match self {
            Aussehen::Hell => "hell",
            Aussehen::Dunkel => "dunkel",
        }
    }
}

/// All single-valued colours of the light appearance as of feature 016 (`main`, `b1315c1`), without the second accent
/// colour `alt-…`. The light appearance stays as it is: every value must stay the light value of its name.
const LIGHT_TODAY: &[(&str, [u8; 3])] = &[
    ("topbar", [0x0b, 0x37, 0x54]),
    ("sidebar", [0x12, 0x7b, 0xb0]),
    ("sidebar-selected", [0x0b, 0x5f, 0x87]),
    ("sidebar-hover", [0x10, 0x6f, 0x9f]),
    ("on-dark", [0xff, 0xff, 0xff]),
    ("surface", [0xf1, 0xf6, 0xf9]),
    ("card", [0xff, 0xff, 0xff]),
    ("card-border", [0xdd, 0xe6, 0xec]),
    ("accent", [0x12, 0x7b, 0xb0]),
    ("power-field", [0x0f, 0x6a, 0x9a]),
    ("tab-idle", [0x5f, 0x6b, 0x75]),
    ("disabled-bg", [0xe3, 0xe8, 0xec]),
    ("disabled-text", [0x56, 0x61, 0x6a]),
    ("focus", [0x0b, 0x37, 0x54]),
    ("button-hover", [0xe8, 0xf4, 0xfb]),
    ("button-hover-text", [0x0f, 0x6a, 0x9a]),
    ("text", [0x1a, 0x1a, 0x1a]),
    ("text-soft", [0x55, 0x55, 0x55]),
    ("free-text", [0x1a, 0x6e, 0x35]),
    ("version-text", [0x8a, 0x5a, 0x00]),
    ("row-hover", [0xe8, 0xf4, 0xfb]),
    ("row-selected", [0xd3, 0xea, 0xf7]),
    ("star-fill", [0xf5, 0xb3, 0x01]),
    ("star-line", [0xa8, 0x74, 0x00]),
    ("star-idle", [0x5f, 0x6b, 0x75]),
    ("tone-ok", [0x2e, 0x9d, 0x4f]),
    ("tone-busy", [0xe8, 0xa3, 0x17]),
    ("tone-alert", [0xc4, 0x30, 0x2b]),
    ("alert-pressed", [0xa3, 0x27, 0x22]),
    ("tone-idle", [0x7a, 0x7a, 0x7a]),
    ("connected", [0x18, 0x81, 0x4d]),
    ("connected-focus", [0x0b, 0x4a, 0x2c]),
    ("card-warning", [0xfd, 0xe7, 0xe6]),
    ("command-bg", [0x1f, 0x1f, 0x1f]),
    ("command-fg", [0xf0, 0xf0, 0xf0]),
];

/// Colours with an alpha value (shadows, dimming) as of the same state, as 8 hex digits: they carry no text.
const LIGHT_TODAY_ALPHA: &[(&str, &str)] = &[
    ("shadow", "0000001a"),
    ("shadow-strong", "00000033"),
    ("scrim", "00000080"),
    ("shadow-dialog", "00000060"),
];

/// (text, background), checked in both appearances at 4.5 : 1.
const PAARE: &[(&str, &str)] = &[
    ("on-dark", "sidebar"),
    ("on-dark", "sidebar-selected"),
    ("on-dark", "sidebar-hover"),
    ("on-dark", "topbar"),
    ("on-dark", "topbar-hover"),
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
    ("accent-fg", "card"),
    ("tab-idle", "card"),
    ("alert-fg", "card"),
    ("alert-fg", "card-warning"),
    ("disabled-text", "disabled-bg"),
    ("free-text", "card"),
    ("free-text", "row-selected"),
    ("free-text", "row-hover"),
    ("version-text", "surface"),
    // Page "Settings"
    ("version-text", "card"),
    ("alert-fg", "surface"),
    ("disabled-text", "card"),
    ("command-fg", "command-bg"),
    // Red main button
    ("on-dark", "alert-pressed"),
    // Donation hint
    ("brand-fg", "card"),
    ("brand-fg", "row-hover"),
    ("alert-fg", "row-hover"),
    // White button and location card while the mouse is over them. `accent` on this tint reaches only 4.18,
    // so text and border turn to `button-hover-text` there.
    ("button-hover-text", "button-hover"),
    ("alert-fg", "button-hover"),
    ("text", "button-hover"),
    ("text-soft", "button-hover"),
    // Green main button
    ("connected-fg", "card"),
    ("connected-fg", "surface"),
    ("on-dark", "connected"),
    ("connected-focus", "card"),
    ("connected-focus", "surface"),
];

/// (control, background) for controls without text, checked in both appearances at 3 : 1. Feature 012: the
/// star on every background of a row. The ring of the state dot against the sidebar (`on-dark` on `sidebar`)
/// is covered by [`PAARE`].
const BEDIENELEMENTE: &[(&str, &str)] = &[
    ("star-line", "card"),
    ("star-line", "row-hover"),
    ("star-line", "row-selected"),
    ("star-idle", "card"),
    ("star-idle", "row-hover"),
    ("star-idle", "row-selected"),
    // Filled blue, red and green areas against the card: switch that is on, main button, field of the main button.
    ("accent", "card"),
    ("tone-alert", "card"),
    ("connected", "card"),
    // Green dot of the connected location in the favourites, on every background of a row.
    ("connected-fg", "row-hover"),
    ("connected-fg", "row-selected"),
    // Focus frame
    ("focus", "card"),
    ("focus", "surface"),
    ("focus", "row-hover"),
    // State dot: fill against its white ring
    ("tone-ok", "on-dark"),
    ("tone-alert", "on-dark"),
    ("tone-idle", "on-dark"),
];

/// (foreground or control, background, limit): pairs that only exist in the light appearance.
const NUR_HELL: &[(&str, &str, f64)] = &[
    // Background of the Celata logo for "Best location" on the rows; in the dark appearance replaced by
    // `image-edge` on the same rows (it reaches only 2.16 to 3.25 there).
    ("accent", "row-hover", MINDESTWERT_BEDIENELEMENT),
    ("accent", "row-selected", MINDESTWERT_BEDIENELEMENT),
    // The prepared second accent colour has a single value.
    ("star-line", "alt-row-hover", MINDESTWERT_BEDIENELEMENT),
    ("star-line", "alt-row-selected", MINDESTWERT_BEDIENELEMENT),
    ("star-idle", "alt-row-hover", MINDESTWERT_BEDIENELEMENT),
    ("star-idle", "alt-row-selected", MINDESTWERT_BEDIENELEMENT),
];

/// (foreground or control, background, limit): pairs that only exist in the dark appearance.
const NUR_DUNKEL: &[(&str, &str, f64)] = &[
    // Border of flags and of the logo of "Best location"; not drawn in the light appearance.
    ("image-edge", "card", MINDESTWERT_BEDIENELEMENT),
    ("image-edge", "row-hover", MINDESTWERT_BEDIENELEMENT),
    ("image-edge", "row-selected", MINDESTWERT_BEDIENELEMENT),
    // Ring of the state dot against the card; in the light appearance the ring on the card is invisible and the
    // fill borders the card directly (covered by "fill against ring").
    ("on-dark", "card", MINDESTWERT_BEDIENELEMENT),
    ("on-dark", "card-warning", MINDESTWERT_BEDIENELEMENT),
];

/// (control, background, limit): pairs that deliberately stay below the limit, in both appearances.
const AUSNAHMEN: &[(&str, &str, f64)] = &[
    // Fill of the yellow-orange state dot ("connecting", "switching") against its white ring: 2.17 : 1 since
    // feature 004. The state text next to the dot carries the meaning, and the light appearance stays as it was.
    ("tone-busy", "on-dark", MINDESTWERT_BEDIENELEMENT),
];

/// The background of the Celata logo (`fill` of its first `rect`) is `accent`, so that the check of
/// `BEDIENELEMENTE` applies to the logo of "Best location"; switching it to `alt-accent` shows up here. The
/// logo does not change with the appearance, so the light value counts.
#[test]
fn logo_grund_ist_accent() {
    let logo = include_str!("../ui/icons/celata.svg");
    let rect = logo
        .lines()
        .find(|line| line.trim_start().starts_with("<rect"))
        .expect("rect im Logo");
    let fill = rect
        .split("fill=\"#")
        .nth(1)
        .and_then(|rest| rest.get(..6))
        .expect("fill im ersten rect");
    let channel = |i: usize| u8::from_str_radix(&fill[i..i + 2], 16).unwrap();
    let hell = farben(Aussehen::Hell);
    assert_eq!([channel(0), channel(2), channel(4)], hell["accent"], "#{fill}");
    for (background, expected) in [("card", 4.68), ("row-hover", 4.18), ("row-selected", 3.76)] {
        let value = kontrast(hell["accent"], hell[background]);
        assert!((value - expected).abs() < 0.01, "{background}: {value:.2}");
    }
}

/// One line `out property <color> name: …;` of `ui/theme.slint`.
struct Zeile {
    name: String,
    /// Everything between `:` and `;`.
    wert: String,
}

fn zeilen() -> Vec<Zeile> {
    THEME
        .lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("out property <color> ")?;
            let (name, wert) = rest.split_once(':')?;
            let wert = wert.trim().strip_suffix(';')?;
            Some(Zeile {
                name: name.trim().to_owned(),
                wert: wert.trim().to_owned(),
            })
        })
        .collect()
}

/// `#rrggbb` or `#rrggbbaa` without the hash, or `None`.
fn hex(text: &str) -> Option<&str> {
    let hex = text.trim().strip_prefix('#')?;
    (matches!(hex.len(), 6 | 8) && hex.bytes().all(|b| b.is_ascii_hexdigit())).then_some(hex)
}

/// (light, dark) of a line. A line without `pick(` has only the light value; `Err` if it cannot be read.
fn werte(zeile: &Zeile) -> Result<(&str, Option<&str>), String> {
    if let Some(inner) = zeile.wert.strip_prefix("pick(").and_then(|rest| rest.strip_suffix(')')) {
        let Some((hell, dunkel)) = inner.split_once(',') else {
            return Err(format!("{}: Wert für das dunkle Aussehen fehlt", zeile.name));
        };
        return match (hex(hell), hex(dunkel)) {
            (Some(hell), Some(dunkel)) => Ok((hell, Some(dunkel))),
            _ => Err(format!("{}: Zeile nicht lesbar ({})", zeile.name, zeile.wert)),
        };
    }
    match hex(&zeile.wert) {
        Some(hell) => Ok((hell, None)),
        None => Err(format!("{}: Zeile nicht lesbar ({})", zeile.name, zeile.wert)),
    }
}

fn rgb(hex: &str) -> Option<[u8; 3]> {
    if hex.len() != 6 {
        return None;
    }
    let channel = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some([channel(0)?, channel(2)?, channel(4)?])
}

/// All colours with 6 hex digits in one appearance. The second accent colour `alt-…` is only in the light one.
/// Colours with an alpha value carry no text and are not in the list.
fn farben(aussehen: Aussehen) -> HashMap<String, [u8; 3]> {
    zeilen()
        .iter()
        .filter_map(|zeile| {
            let (hell, dunkel) = werte(zeile).ok()?;
            let hex = match aussehen {
                Aussehen::Hell => hell,
                Aussehen::Dunkel => dunkel?,
            };
            Some((zeile.name.clone(), rgb(hex)?))
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

/// Every colour has a value for both appearances: a line with one value, a missing second value or a
/// line that cannot be read fails here, with the name. Only the second accent colour `alt-…` has one value.
#[test]
fn jede_farbe_hat_beide_aussehen() {
    let mut fehler = Vec::new();
    let mut gelesen = 0;
    for zeile in zeilen() {
        gelesen += 1;
        match werte(&zeile) {
            Err(meldung) => fehler.push(meldung),
            Ok((_, None)) if !zeile.name.starts_with("alt-") => {
                fehler.push(format!("{}: Wert für das dunkle Aussehen fehlt", zeile.name));
            }
            Ok((_, Some(_))) if zeile.name.starts_with("alt-") => {
                fehler.push(format!("{}: die Zweitfarbe hat nur einen Wert", zeile.name));
            }
            Ok(_) => {}
        }
    }
    assert!(gelesen >= 45, "zu wenige Farbzeilen gelesen: {gelesen}");
    assert!(fehler.is_empty(), "{fehler:?}");
}

/// The light appearance stays as it was: every name from [`LIGHT_TODAY`] still has its light value.
#[test]
fn helles_aussehen_ist_unveraendert() {
    let zeilen = zeilen();
    let hell = |name: &str| -> Option<&str> {
        let zeile = zeilen.iter().find(|zeile| zeile.name == name)?;
        werte(zeile).ok().map(|(hell, _)| hell)
    };
    let mut abweichung = Vec::new();
    for (name, rgb_heute) in LIGHT_TODAY {
        let heute = format!("{:02x}{:02x}{:02x}", rgb_heute[0], rgb_heute[1], rgb_heute[2]);
        match hell(name) {
            Some(wert) if wert.eq_ignore_ascii_case(&heute) => {}
            Some(wert) => abweichung.push(format!("{name}: hell war #{heute}, ist #{wert}")),
            None => abweichung.push(format!("{name}: fehlt oder nicht lesbar")),
        }
    }
    for (name, heute) in LIGHT_TODAY_ALPHA {
        match hell(name) {
            Some(wert) if wert.eq_ignore_ascii_case(heute) => {}
            Some(wert) => abweichung.push(format!("{name}: hell war #{heute}, ist #{wert}")),
            None => abweichung.push(format!("{name}: fehlt oder nicht lesbar")),
        }
    }
    assert_eq!(
        LIGHT_TODAY.len() + LIGHT_TODAY_ALPHA.len(),
        39,
        "die Liste von heute hat 39 Farben"
    );
    assert!(abweichung.is_empty(), "helles Aussehen verändert: {abweichung:?}");
}

/// A foreground colour has the same value as its area in the light appearance, so the light picture stays as it
/// was after the split of the roles.
#[test]
fn vordergrundfarben_sind_hell_gleich_ihrer_flaeche() {
    let hell = farben(Aussehen::Hell);
    for (vordergrund, flaeche) in [
        ("accent-fg", "accent"),
        ("alert-fg", "tone-alert"),
        ("connected-fg", "connected"),
        ("brand-fg", "topbar"),
    ] {
        assert_eq!(
            hell[vordergrund], hell[flaeche],
            "{vordergrund} ist hell nicht gleich {flaeche}"
        );
    }
}

fn pruefe(aussehen: Aussehen, paare: &[(&str, &str, f64)]) -> Vec<String> {
    let farben = farben(aussehen);
    let mut schwach = Vec::new();
    for (vorne, hinten, grenze) in paare {
        let (Some(a), Some(b)) = (farben.get(*vorne), farben.get(*hinten)) else {
            panic!("{vorne} oder {hinten} fehlt in ui/theme.slint ({})", aussehen.name());
        };
        let wert = kontrast(*a, *b);
        if wert < *grenze {
            schwach.push(format!("{}: {vorne} auf {hinten}: {wert:.2}", aussehen.name()));
        }
    }
    schwach
}

#[test]
fn jede_genannte_farbe_steht_im_farbschema() {
    let zeilen = zeilen();
    let namen: HashSet<&str> = zeilen.iter().map(|zeile| zeile.name.as_str()).collect();
    let genannt = PAARE.iter().chain(BEDIENELEMENTE).copied().chain(
        NUR_HELL
            .iter()
            .chain(NUR_DUNKEL)
            .chain(AUSNAHMEN)
            .map(|(a, b, _)| (*a, *b)),
    );
    for (vorne, hinten) in genannt {
        assert!(namen.contains(vorne), "{vorne} fehlt in ui/theme.slint");
        assert!(namen.contains(hinten), "{hinten} fehlt in ui/theme.slint");
    }
}

#[test]
fn alle_farbpaare_sind_gut_lesbar() {
    let paare: Vec<(&str, &str, f64)> = PAARE.iter().map(|(a, b)| (*a, *b, MINDESTWERT)).collect();
    let schwach: Vec<String> = Aussehen::BEIDE.iter().flat_map(|a| pruefe(*a, &paare)).collect();
    assert!(schwach.is_empty(), "Kontrast unter {MINDESTWERT}: {schwach:?}");
}

#[test]
fn bedienelemente_heben_sich_ab() {
    let paare: Vec<(&str, &str, f64)> = BEDIENELEMENTE
        .iter()
        .map(|(a, b)| (*a, *b, MINDESTWERT_BEDIENELEMENT))
        .collect();
    let schwach: Vec<String> = Aussehen::BEIDE.iter().flat_map(|a| pruefe(*a, &paare)).collect();
    assert!(
        schwach.is_empty(),
        "Kontrast unter {MINDESTWERT_BEDIENELEMENT}: {schwach:?}"
    );
}

#[test]
fn paare_eines_aussehens_sind_lesbar() {
    let schwach: Vec<String> = pruefe(Aussehen::Hell, NUR_HELL)
        .into_iter()
        .chain(pruefe(Aussehen::Dunkel, NUR_DUNKEL))
        .collect();
    assert!(schwach.is_empty(), "Kontrast unter der Grenze: {schwach:?}");
}

/// Every pair is in exactly one list; every name of the special lists is in `ui/theme.slint`; every exception is
/// really below its limit in each appearance (otherwise it is superfluous and has to be removed).
#[test]
fn listen_der_paare_sind_stimmig() {
    let mut gesehen: HashSet<(&str, &str)> = HashSet::new();
    let alle = PAARE.iter().chain(BEDIENELEMENTE).copied().chain(
        NUR_HELL
            .iter()
            .chain(NUR_DUNKEL)
            .chain(AUSNAHMEN)
            .map(|(a, b, _)| (*a, *b)),
    );
    for paar in alle {
        assert!(gesehen.insert(paar), "{paar:?} steht in mehr als einer Liste");
    }
    // A pair of the light-only list with a name `alt-…` is the only reason for a name without dark value.
    for (a, b, _) in NUR_HELL.iter().chain(NUR_DUNKEL).chain(AUSNAHMEN) {
        for name in [a, b] {
            assert!(
                THEME.contains(&format!("<color> {name}:")),
                "{name} kommt in ui/theme.slint nicht vor"
            );
        }
    }
    for aussehen in Aussehen::BEIDE {
        let farben = farben(aussehen);
        for (vorne, hinten, grenze) in AUSNAHMEN {
            let wert = kontrast(farben[*vorne], farben[*hinten]);
            assert!(
                wert < *grenze,
                "Ausnahme überflüssig: {} {vorne} auf {hinten} erreicht {wert:.2}",
                aussehen.name()
            );
        }
    }
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

/// The prepared second accent colour passes the same pairs (light appearance), so that switching needs no
/// further check. A foreground colour that equals its area in the light appearance follows the area.
#[test]
fn second_accent_colour_is_readable_too() {
    let farben = farben(Aussehen::Hell);
    let variante = |name: &str| -> [u8; 3] {
        let name = match name {
            "accent-fg" => "accent",
            "brand-fg" => "topbar",
            other => other,
        };
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

/// The code of a line without comment and without the content of string literals.
fn code_ohne_text(line: &str) -> String {
    let code = line.split("//").next().unwrap_or_default();
    let mut aus = String::new();
    let mut in_text = false;
    for zeichen in code.chars() {
        if zeichen == '"' {
            in_text = !in_text;
        } else if !in_text {
            aus.push(zeichen);
        }
    }
    aus
}

/// Colours live only in `ui/theme.slint`: no other `.slint` file contains a colour value, a colour
/// name (`white`, `black`, `Colors.…`) or a colour function. `transparent` is allowed.
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
            let code = code_ohne_text(line);
            let literal = code.split('#').skip(1).any(|rest| {
                let digits = rest.bytes().take_while(u8::is_ascii_hexdigit).count();
                matches!(digits, 3 | 4 | 6 | 8)
            });
            let namen = code
                .split(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '.' || c == '_'))
                .any(|wort| wort == "white" || wort == "black" || wort.starts_with("Colors."));
            let funktion = ["rgb(", "rgba(", "hsv("].iter().any(|f| {
                code.match_indices(f).any(|(i, _)| {
                    !code[..i]
                        .chars()
                        .next_back()
                        .is_some_and(|c| c.is_ascii_alphanumeric() || c == '-')
                })
            });
            if literal || namen || funktion {
                found.push(format!("ui/{name}:{}", number + 1));
            }
        }
    }
    assert!(checked >= 5, "zu wenige .slint-Dateien gefunden");
    assert!(found.is_empty(), "Farbwerte außerhalb von ui/theme.slint: {found:?}");
}
