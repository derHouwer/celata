// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! Command-line handling: version, help, unblocking without the window, the rule for polkit and the start at
//! login. Also the message for a start without a graphical session, which can only go to the terminal.

use crate::i18n::{self, Key, Language, Phrase};
use crate::text;

/// Longest technical cause shown after the message for a missing graphical session.
const MAX_CAUSE: usize = 500;

/// Longest unknown argument repeated in the message about it.
const MAX_ARGUMENT: usize = 100;

/// Exit code for an unknown option, as usual for wrong use of a command.
pub const EXIT_UNKNOWN: u8 = 2;

/// What the command line asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// The window. `autostart`: started by the entry for the start at login; never connects.
    Gui {
        autostart: bool,
    },
    Version,
    Help,
    /// Unblock the internet without the window.
    Unblock,
    /// Print the rule for polkit that lets members of the group start the service.
    PolkitRule,
    /// Anything else, with the argument cleaned and cut for the message.
    Unknown(String),
}

/// Reads the arguments including the program name as the first element. No argument or exactly one known
/// argument; every other form is `Unknown`. `--autostart` is accepted but not listed in the help: only the
/// entry for the start at login uses it.
pub fn parse<I, S>(args: I) -> Command
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut args = args.into_iter().skip(1);
    let Some(first) = args.next() else {
        return Command::Gui { autostart: false };
    };
    let first = first.as_ref();
    if let Some(second) = args.next() {
        return Command::Unknown(text::printable(&format!("{first} {}", second.as_ref()), MAX_ARGUMENT));
    }
    match first {
        "--version" | "-V" => Command::Version,
        "--help" | "-h" => Command::Help,
        "--unblock" => Command::Unblock,
        "--polkit-rule" => Command::PolkitRule,
        "--autostart" => Command::Gui { autostart: true },
        other => Command::Unknown(text::printable(other, MAX_ARGUMENT)),
    }
}

/// Help for `celata --help`.
pub fn help_text(language: Language) -> String {
    i18n::text(language, Key::CliHelp).to_owned()
}

/// Message for an unknown option, with a pointer to the help.
pub fn unknown_text(language: Language, argument: &str) -> String {
    Phrase::fill(Key::CliUnknown, [Phrase::raw(argument)]).render(language)
}

/// Version of Celata, defined once in `[workspace.package]` of `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The line `celata --version` prints, in the usual form `<program> <version>`.
#[must_use]
pub fn version_line() -> String {
    format!("celata {VERSION}")
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
        assert_eq!(version_line(), format!("celata {}", env!("CARGO_PKG_VERSION")));
    }

    fn parse_args(args: &[&str]) -> Command {
        parse(std::iter::once("celata").chain(args.iter().copied()))
    }

    #[test]
    fn no_argument_opens_the_window() {
        assert_eq!(parse_args(&[]), Command::Gui { autostart: false });
        assert_eq!(parse(Vec::<String>::new()), Command::Gui { autostart: false });
    }

    #[test]
    fn known_arguments_are_recognised() {
        assert_eq!(parse_args(&["--autostart"]), Command::Gui { autostart: true });
        assert_eq!(parse_args(&["--version"]), Command::Version);
        assert_eq!(parse_args(&["-V"]), Command::Version);
        assert_eq!(parse_args(&["--help"]), Command::Help);
        assert_eq!(parse_args(&["-h"]), Command::Help);
        assert_eq!(parse_args(&["--unblock"]), Command::Unblock);
        assert_eq!(parse_args(&["--polkit-rule"]), Command::PolkitRule);
    }

    #[test]
    fn program_name_is_skipped() {
        assert_eq!(parse(["--version"]), Command::Gui { autostart: false });
        assert_eq!(parse(["/usr/bin/celata", "--unblock"]), Command::Unblock);
        assert_eq!(parse(["/usr/bin/celata", "--polkit-rule"]), Command::PolkitRule);
    }

    #[test]
    fn everything_else_is_unknown() {
        for args in [
            vec!["--version", "--version"],
            vec!["--version", "x"],
            vec!["x", "--version"],
            vec!["--Version"],
            vec!["-v"],
            vec!["version"],
            vec!["--unblock=x"],
            vec!["--polkit-rule", "x"],
            vec!["--polkit"],
            vec![""],
            vec!["--x"],
        ] {
            assert!(matches!(parse_args(&args), Command::Unknown(_)), "{args:?}");
        }
        assert_eq!(parse_args(&["--x"]), Command::Unknown("--x".into()));
        assert_eq!(
            parse_args(&["--unblock", "now"]),
            Command::Unknown("--unblock now".into())
        );
    }

    #[test]
    fn unknown_argument_is_cleaned_and_cut() {
        let long = format!("--{}\n\u{1b}[31m", "x".repeat(300));
        let Command::Unknown(shown) = parse_args(&[&long]) else {
            panic!("unknown expected");
        };
        assert_eq!(shown.chars().count(), 100);
        assert!(!shown.chars().any(char::is_control), "{shown:?}");
        assert_eq!(parse_args(&["a\nb"]), Command::Unknown("ab".into()));
    }

    #[test]
    fn help_names_the_public_options_in_both_languages() {
        for language in Language::ALL {
            let help = help_text(language);
            assert!(help.contains("--version"), "{help}");
            assert!(help.contains("--unblock"), "{help}");
            assert!(help.contains("--polkit-rule"), "{help}");
            assert!(help.contains("--help"), "{help}");
            assert!(!help.contains("--autostart"), "{help}");
        }
        assert!(help_text(Language::De).starts_with("Aufruf: celata"));
        assert!(
            help_text(Language::En).contains("--polkit-rule  Print the rule that lets members of the group celata")
        );
        assert!(
            help_text(Language::De).contains("--polkit-rule  Gibt die Regel aus, mit der Mitglieder der Gruppe celata")
        );
    }

    #[test]
    fn unknown_option_points_to_the_help() {
        assert_eq!(
            unknown_text(Language::En, "--x"),
            "Unknown option: --x. See celata --help."
        );
        assert_eq!(
            unknown_text(Language::De, "--x"),
            "Unbekannte Option: --x. Siehe celata --help."
        );
        assert_eq!(EXIT_UNKNOWN, 2);
    }
}
