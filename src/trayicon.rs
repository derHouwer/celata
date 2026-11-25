// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! Logo and hover text for the icon in the system tray and for the window icon.
//!
//! The system tray takes only pixel images, no SVG. This module therefore draws the logo from
//! `ui/icons/celata.svg` itself, with the base colour of the connection state instead of the blue of the
//! sidebar. While connected, the white arc is closed to a full ring around the core, so that "Connected"
//! does not depend on the colour alone (blue and green are equally light). The hover text names Celata and
//! the short state, never a host name or an address.

use slint::{Rgba8Pixel, SharedPixelBuffer};

use crate::desktop;
use crate::i18n::{self, Key, Language, Phrase};
use crate::look::{self, StateColor};
use crate::servers::Country;
use crate::state::StatusView;
use crate::streaming;

/// Base colour of the logo while not connected: the light value of `sidebar` in `ui/theme.slint`. The icon in the
/// system tray does not follow the appearance of the window (light or dark).
pub const BLUE: [u8; 3] = [0x12, 0x7b, 0xb0];
/// Base colour while connected: the light value of `connected` in `ui/theme.slint`.
pub const GREEN: [u8; 3] = [0x18, 0x81, 0x4d];
/// Base colour while the internet is blocked: the light value of `tone-alert` in `ui/theme.slint`.
pub const RED: [u8; 3] = [0xc4, 0x30, 0x2b];

/// The base colour as it stands in the logo file, exactly once.
const BASE_IN_FILE: &str = "#127bb0";

/// The open arc as it stands in the logo file, exactly once.
const ARC_IN_FILE: &str = r#"d="M45.4 21.3 A17 17 0 1 0 45.4 42.7""#;
/// The same arc closed to a full ring: same centre, radius and stroke width.
const RING: &str = r#"d="M49 32 A17 17 0 1 0 15 32 A17 17 0 1 0 49 32 Z""#;

/// Width and height of the pixel image. The desktop scales it down to the size of the system tray.
pub const SIZE: u32 = 64;

/// Base colour of the logo for the state.
fn color(state: StateColor) -> [u8; 3] {
    match state {
        StateColor::Normal => BLUE,
        StateColor::Connected => GREEN,
        StateColor::Blocked => RED,
    }
}

/// The logo in the colour of the state as a pixel image of 64 × 64. `None` if the built-in file cannot be
/// drawn (never expected); the caller then keeps the previous icon without a message.
pub fn render(state: StateColor) -> Option<SharedPixelBuffer<Rgba8Pixel>> {
    let [r, g, b] = color(state);
    let mut svg = std::str::from_utf8(desktop::ICON_SVG)
        .ok()?
        .replace(BASE_IN_FILE, &format!("#{r:02x}{g:02x}{b:02x}"));
    if state == StateColor::Connected {
        svg = svg.replace(ARC_IN_FILE, RING);
    }
    let tree = resvg::usvg::Tree::from_str(&svg, &resvg::usvg::Options::default()).ok()?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(SIZE, SIZE)?;
    #[allow(clippy::cast_precision_loss)] // 64 is exact as f32
    let scale = SIZE as f32 / tree.size().width();
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::new(SIZE, SIZE);
    for (target, source) in buffer.make_mut_slice().iter_mut().zip(pixmap.pixels()) {
        // tiny-skia stores premultiplied alpha, Slint expects straight alpha.
        let pixel = source.demultiply();
        *target = Rgba8Pixel {
            r: pixel.red(),
            g: pixel.green(),
            b: pixel.blue(),
            a: pixel.alpha(),
        };
    }
    Some(buffer)
}

/// Short state for the hover text: the title of the state as at the bottom of the sidebar. During a
/// switch the target stands there as location ("Netherlands, Amsterdam" or "Streaming Japan"), never as
/// host name; if the location is unknown, without a target.
pub fn short_state(view: &StatusView, countries: &[Country], language: Language) -> String {
    // While connecting the target is named like during a switch (feature 013).
    let Some(target) = view.switch_target.as_ref().or(view.connect_target.as_ref()) else {
        return i18n::text(language, view.message.title).to_owned();
    };
    // Streaming servers as in the sidebar ("Streaming Japan"), other servers with their location.
    let location = streaming::describe_host(target)
        .map(|phrase| phrase.render(language))
        .or_else(|| look::location_of(target, countries, language).map(|(_, label)| label));
    match location {
        Some(location) => Phrase::fill(view.message.title, [Phrase::raw(location)]).render(language),
        None if view.switch_target.is_some() => i18n::text(language, Key::ButtonSwitching).to_owned(),
        None => i18n::text(language, Key::ButtonConnecting).to_owned(),
    }
}

/// Hover text of the icon: "Celata – <short state>".
pub fn tooltip(language: Language, short: &str) -> String {
    i18n::fill(i18n::text(language, Key::TrayTooltip), &[short.to_owned()])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::{Error, Observation, ServiceState};
    use crate::servers::parse_list;
    use crate::state::Monitor;

    const THEME: &str = include_str!("../ui/theme.slint");
    const FIXTURE: &str = include_str!("../tests/fixtures/servers.json");

    /// The light value of colour `name` from `ui/theme.slint`, format `out property <color> name: pick(#light, #dark);`.
    fn theme_color(name: &str) -> [u8; 3] {
        let prefix = format!("out property <color> {name}:");
        let line = THEME
            .lines()
            .map(str::trim)
            .find(|line| line.starts_with(&prefix))
            .unwrap_or_else(|| panic!("{name} fehlt in ui/theme.slint"));
        let value = line[prefix.len()..].trim();
        let light = value
            .strip_prefix("pick(")
            .unwrap_or_else(|| panic!("{name}: Format pick(#hell, #dunkel) erwartet"))
            .split(',')
            .next()
            .unwrap_or_default();
        let hex = light.trim().trim_start_matches('#');
        let channel = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).unwrap();
        [channel(0), channel(2), channel(4)]
    }

    #[test]
    fn grundfarbe_steht_genau_einmal_im_logo() {
        let svg = std::str::from_utf8(desktop::ICON_SVG).unwrap();
        assert_eq!(svg.matches(BASE_IN_FILE).count(), 1);
    }

    #[test]
    fn farben_stimmen_mit_dem_farbschema_ueberein() {
        assert_eq!(theme_color("sidebar"), BLUE);
        assert_eq!(theme_color("connected"), GREEN);
        assert_eq!(theme_color("tone-alert"), RED);
    }

    #[test]
    fn logo_in_jeder_zustandsfarbe() {
        let core = theme_color("topbar");
        for state in [StateColor::Normal, StateColor::Connected, StateColor::Blocked] {
            let image = render(state).expect("Logo zeichenbar");
            assert_eq!((image.width(), image.height()), (SIZE, SIZE));
            let pixel = |x: usize, y: usize| image.as_slice()[y * SIZE as usize + x];
            let close = |pixel: Rgba8Pixel, rgb: [u8; 3]| {
                pixel.a == 255
                    && [pixel.r, pixel.g, pixel.b]
                        .iter()
                        .zip(rgb)
                        .all(|(have, want)| have.abs_diff(want) <= 2)
            };
            // Base area on the left, outside the arc
            assert!(close(pixel(6, 32), color(state)), "{state:?}: {:?}", pixel(6, 32));
            // Core in the middle
            assert!(close(pixel(32, 32), core), "{state:?}: {:?}", pixel(32, 32));
            // Arc at the top, white (radius 17 around the middle)
            assert!(close(pixel(32, 15), [255, 255, 255]), "{state:?}: {:?}", pixel(32, 15));
            // Opening of the arc on the right: closed to a ring only while connected
            let opening = if state == StateColor::Connected {
                [255, 255, 255]
            } else {
                color(state)
            };
            assert!(close(pixel(49, 32), opening), "{state:?}: {:?}", pixel(49, 32));
        }
    }

    #[test]
    fn bogen_steht_genau_einmal_im_logo() {
        let svg = std::str::from_utf8(desktop::ICON_SVG).unwrap();
        assert_eq!(svg.matches(ARC_IN_FILE).count(), 1);
    }

    #[test]
    fn hinweis_in_beiden_sprachen() {
        assert_eq!(tooltip(Language::En, "Connected"), "Celata – Connected");
        assert_eq!(tooltip(Language::De, "Verbunden"), "Celata – Verbunden");
    }

    fn observe(monitor: &mut Monitor, code: &str, host: Option<&str>, blocked: bool) {
        let state = ServiceState {
            code: code.to_owned(),
            host: host.map(str::to_owned),
            ..ServiceState::default()
        };
        monitor.observe(Observation::State { state, blocked });
    }

    /// Monitors in every state the sidebar can show, including switches to a normal and to a streaming
    /// server.
    fn monitors() -> Vec<Monitor> {
        let mut list = Vec::new();
        list.push(Monitor::new());
        for (code, host, blocked) in [
            ("clean", None, false),
            ("routed", None, true),
            ("connecting", None, true),
            ("connecting", None, false),
            ("connected", Some("nl-v4.hideservers.net"), true),
            ("connected", Some("streaming-jp.hideservers.net"), true),
        ] {
            let mut monitor = Monitor::new();
            observe(&mut monitor, code, host, blocked);
            list.push(monitor);
        }
        for target in [
            "de-v4.hideservers.net",
            "streaming-jp.hideservers.net",
            "unknown.hideservers.net",
        ] {
            let mut monitor = Monitor::new();
            observe(&mut monitor, "connected", Some("nl-v4.hideservers.net"), true);
            monitor.begin_connect(target).unwrap();
            assert!(monitor.view().switch_target.is_some());
            list.push(monitor);
        }
        // Connecting with a target, also from the block and to an unknown host (feature 013).
        for (code, blocked, target) in [
            ("clean", false, "de-v4.hideservers.net"),
            ("routed", true, "de-v4.hideservers.net"),
            ("clean", false, "unknown.hideservers.net"),
        ] {
            let mut monitor = Monitor::new();
            observe(&mut monitor, code, None, blocked);
            monitor.begin_connect(target).unwrap();
            assert!(monitor.view().connect_target.is_some());
            list.push(monitor);
        }
        let mut disconnecting = Monitor::new();
        observe(&mut disconnecting, "connected", Some("nl-v4.hideservers.net"), true);
        disconnecting.begin_disconnect(false);
        list.push(disconnecting);
        let mut reapplying = Monitor::new();
        observe(&mut reapplying, "connected", Some("nl-v4.hideservers.net"), true);
        reapplying.begin_reapply().unwrap();
        list.push(reapplying);
        for (error, blocked) in [
            (Error::NotRunning, false),
            (Error::NotRunning, true),
            (Error::PermissionDenied, false),
        ] {
            let mut monitor = Monitor::new();
            monitor.observe(Observation::Failed { error, blocked });
            list.push(monitor);
        }
        list
    }

    /// Contains something like an IPv4 address (four groups of digits separated by dots) or an IPv6
    /// address (several groups separated by colons).
    fn looks_like_address(text: &str) -> bool {
        text.split(|c: char| !(c.is_ascii_hexdigit() || c == '.' || c == ':'))
            .any(|word| {
                let dotted = word.split('.').filter(|part| !part.is_empty()).count() >= 4
                    && word.split('.').all(|part| part.chars().all(|c| c.is_ascii_digit()));
                let colons = word.matches(':').count() >= 2;
                dotted || colons
            })
    }

    #[test]
    fn hinweis_ohne_hostnamen_und_adressen() {
        let countries = parse_list(FIXTURE.as_bytes()).unwrap();
        assert!(looks_like_address("Celata – 10.0.0.1"));
        assert!(looks_like_address("Celata – fe80::1"));
        for monitor in monitors() {
            for list in [&countries[..], &[]] {
                for language in Language::ALL {
                    let text = tooltip(language, &short_state(&monitor.view(), list, language));
                    assert!(!text.contains("hideservers.net"), "{text}");
                    assert!(!looks_like_address(&text), "{text}");
                    assert!(text.starts_with("Celata – "), "{text}");
                }
            }
        }
    }

    #[test]
    fn hinweis_beim_wechsel_nennt_das_ziel_als_ort() {
        let countries = parse_list(FIXTURE.as_bytes()).unwrap();
        let mut monitor = Monitor::new();
        observe(&mut monitor, "connected", Some("nl-v4.hideservers.net"), true);
        monitor.begin_connect("streaming-jp.hideservers.net").unwrap();
        let short = short_state(&monitor.view(), &countries, Language::De);
        assert!(short.contains("Streaming Japan"), "{short}");
    }

    #[test]
    fn hinweis_beim_verbinden_nennt_das_ziel_als_ort() {
        let countries = parse_list(FIXTURE.as_bytes()).unwrap();
        let mut monitor = Monitor::new();
        observe(&mut monitor, "clean", None, false);
        monitor.begin_connect("berlin.hideservers.net").unwrap();
        let view = monitor.view();
        assert_eq!(
            short_state(&view, &countries, Language::De),
            "Verbinde mit Deutschland, Berlin …"
        );
        assert_eq!(
            tooltip(Language::En, &short_state(&view, &countries, Language::En)),
            "Celata – Connecting to Germany, Berlin …"
        );
        // Unknown target: without a target, never with the host name.
        assert_eq!(short_state(&view, &[], Language::De), "Wird verbunden …");
    }

    #[test]
    fn zustandstexte_der_seitenleiste_ohne_hostnamen() {
        for key in Key::ALL {
            let name = format!("{key:?}");
            if !(name.starts_with("State") && name.ends_with("Title")) {
                continue;
            }
            for language in Language::ALL {
                let text = i18n::text(language, *key);
                assert!(!text.contains("hideservers.net"), "{name}: {text}");
                assert!(!looks_like_address(text), "{name}: {text}");
            }
        }
    }
}
