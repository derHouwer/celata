// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! Command-line handling: Celata has no options besides printing its version. Also the message for a
//! start without a graphical session, which can only go to the terminal.

use crate::i18n::{Key, Phrase};
use crate::text;

/// Longest technical cause shown after the message for a missing graphical session.
const MAX_CAUSE: usize = 500;

/// Version of Celata, defined once in `[workspace.package]` of `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The line `celata --version` prints, in the usual form `<program> <version>`.
#[must_use]
pub fn version_line() -> String {
    format!("celata {VERSION}")
}

/// True when the arguments after the program name are exactly `--version` or `-V`.
pub fn wants_version<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut args = args.into_iter();
    let (Some(first), None) = (args.next(), args.next()) else {
        return false;
    };
    matches!(first.as_ref(), "--version" | "-V")
}

/// Message for the terminal when the window cannot be opened: a sentence the user understands, followed
/// by the technical cause from the windowing system.
#[must_use]
pub fn no_session_message(cause: &str) -> Phrase {
    Phrase::with_reason(
        Phrase::key(Key::NoGraphicalSession),
        Phrase::raw(text::printable(cause, MAX_CAUSE)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Language;

    #[test]
    fn missing_session_is_explained_in_both_languages_with_the_cause() {
        let message = no_session_message("neither WAYLAND_DISPLAY nor DISPLAY is set");
        let english = message.render(Language::En);
        assert_eq!(
            english,
            "Celata needs a graphical session (Wayland, or X11 untested). Reason: neither WAYLAND_DISPLAY nor \
             DISPLAY is set"
        );
        let german = message.render(Language::De);
        assert!(
            german.starts_with("Celata braucht eine grafische Sitzung (Wayland, oder X11 ungetestet)."),
            "{german}"
        );
        assert!(
            german.ends_with("Grund: neither WAYLAND_DISPLAY nor DISPLAY is set"),
            "{german}"
        );
    }

    #[test]
    fn cause_is_shown_without_control_characters() {
        let message = no_session_message("line one\nline two\u{1b}[0m").render(Language::En);
        assert!(message.ends_with("Reason: line oneline two[0m"), "{message}");
    }

    #[test]
    fn version_line_names_program_and_version() {
        assert_eq!(version_line(), "celata 0.8.1");
        assert_eq!(version_line(), format!("celata {}", env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn version_is_requested_by_exactly_one_known_argument() {
        assert!(wants_version(["--version"]));
        assert!(wants_version(["-V"]));
    }

    #[test]
    fn version_is_not_requested_otherwise() {
        assert!(!wants_version(Vec::<String>::new()));
        assert!(!wants_version(["--help"]));
        assert!(!wants_version(["-v"]));
        assert!(!wants_version(["version"]));
        assert!(!wants_version(["--version", "--version"]));
        assert!(!wants_version(["--version", "x"]));
        assert!(!wants_version(["x", "--version"]));
        assert!(!wants_version([""]));
    }
}
