// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! Cleaning of text that comes from outside (service, public lists) before it is shown.

/// `text` without control characters, line separators and surrounding whitespace, cut to at most
/// `max_chars` characters.
#[must_use]
pub fn printable(text: &str, max_chars: usize) -> String {
    let cleaned: String = text
        .chars()
        .filter(|character| !character.is_control() && !matches!(character, '\u{2028}' | '\u{2029}'))
        .collect();
    cleaned.trim().chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_stays_as_it_is() {
        assert_eq!(printable("nl.hideservers.net", 253), "nl.hideservers.net");
        assert_eq!(printable("São Paulo #2", 64), "São Paulo #2");
        assert_eq!(printable("", 10), "");
    }

    #[test]
    fn control_characters_are_removed() {
        assert_eq!(printable("a\nb\tc\r\u{1b}[31md\u{0}\u{7f}", 64), "abc[31md");
        assert_eq!(printable("line one\u{2028}two\u{2029}", 64), "line onetwo");
    }

    #[test]
    fn surrounding_whitespace_is_trimmed() {
        assert_eq!(printable("  Milan  ", 64), "Milan");
        assert_eq!(printable("\n\n", 64), "");
    }

    #[test]
    fn text_is_cut_by_characters_not_bytes() {
        assert_eq!(printable("ääääää", 3), "äää");
        assert_eq!(printable(&"x".repeat(1000), 300).chars().count(), 300);
        assert_eq!(printable("abc", 0), "");
    }
}
