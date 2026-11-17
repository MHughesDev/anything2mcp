//! Text rules: Unicode NFC, whitespace, and case.
//!
//! Every rule here is fixed by the Unicode tables compiled into this build,
//! never by the machine's locale.

use unicode_normalization::UnicodeNormalization;

/// The text in Unicode normalization form C.
#[must_use]
pub fn nfc(text: &str) -> String {
    text.nfc().collect()
}

/// The text without whitespace at either end. A non-breaking space and a
/// narrow no-break space are whitespace.
#[must_use]
pub fn trim(text: &str) -> &str {
    text.trim()
}

/// The text with every run of whitespace turned into one space, and none at
/// either end. A non-breaking space and a narrow no-break space are whitespace.
#[must_use]
pub fn collapse_space(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for word in text.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}

/// The text in lower case, by the Unicode rule that needs no locale.
#[must_use]
pub fn lower(text: &str) -> String {
    text.to_lowercase()
}

/// The text in upper case, by the Unicode rule that needs no locale.
#[must_use]
pub fn upper(text: &str) -> String {
    text.to_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nfc_composes_and_is_stable() {
        // "e" followed by a combining acute accent is one composed letter.
        assert_eq!(nfc("cafe\u{301}"), "caf\u{e9}");
        assert_eq!(nfc("caf\u{e9}"), "caf\u{e9}");
        // The angstrom sign and the letter it stands for are one character.
        assert_eq!(nfc("\u{212B}"), "\u{C5}");
        assert_eq!(nfc(""), "");
    }

    #[test]
    fn whitespace_includes_the_no_break_spaces() {
        assert_eq!(trim("\u{A0} a b\u{202F}\t"), "a b");
        assert_eq!(collapse_space("  a\u{A0}\u{A0}b \u{202F} c\r\n"), "a b c");
        assert_eq!(collapse_space("\u{A0}"), "");
        assert_eq!(collapse_space("a"), "a");
    }

    #[test]
    fn case_needs_no_locale() {
        assert_eq!(lower("STRASSE \u{130}"), "strasse i\u{307}");
        assert_eq!(upper("stra\u{df}e"), "STRASSE");
        assert_eq!(lower("ABC"), "abc");
    }
}
