// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! What the dialog "About Celata" shows, and the setup guide the status card links to. Both addresses are
//! opened with `xdg-open` like the donation link, never fetched by Celata itself.

use crate::cli;
use crate::i18n::{self, Key, Language};

/// Public page of the project.
pub const PROJECT_URL: &str = "https://github.com/derHouwer/celata";

/// Guide to setting up the service by hand, including the rule for polkit; linked from the status card.
pub const SETUP_GUIDE_URL: &str = "https://github.com/derHouwer/celata/blob/main/docs/manual-setup.md";

/// Licence of Celata, as in `Cargo.toml`.
pub const LICENSE: &str = "GPL-3.0-only";

/// "Version <number>".
pub fn version_text(language: Language) -> String {
    i18n::fill(i18n::text(language, Key::AboutVersion), &[cli::VERSION.to_owned()])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_page_is_the_public_repository() {
        assert_eq!(PROJECT_URL, "https://github.com/derHouwer/celata");
        assert!(PROJECT_URL.starts_with("https://"));
        assert!(!PROJECT_URL.contains(concat!("celata", "-dev")));
    }

    #[test]
    fn setup_guide_is_in_the_public_repository() {
        assert_eq!(
            SETUP_GUIDE_URL,
            "https://github.com/derHouwer/celata/blob/main/docs/manual-setup.md"
        );
        assert!(SETUP_GUIDE_URL.starts_with(&format!("{PROJECT_URL}/")));
        assert!(!SETUP_GUIDE_URL.contains(concat!("celata", "-dev")));
    }

    #[test]
    fn licence_matches_the_package() {
        assert_eq!(LICENSE, "GPL-3.0-only");
        assert_eq!(LICENSE, env!("CARGO_PKG_LICENSE"));
    }

    #[test]
    fn version_line_in_both_languages() {
        for language in Language::ALL {
            let text = version_text(language);
            assert!(text.contains(cli::VERSION), "{text}");
            assert!(text.starts_with("Version "), "{text}");
        }
        assert_eq!(version_text(Language::En), format!("Version {}", cli::VERSION));
    }
}
