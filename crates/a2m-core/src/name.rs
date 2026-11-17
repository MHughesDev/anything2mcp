//! Checked names, and the two ways a name can be wrong.

use std::fmt;

/// The longest name shown back in an error. A name comes from a file or a
/// caller and may be of any length.
const SHOWN: usize = 64;

/// A name that is not one of a closed vocabulary's values.
///
/// It maps to the failure class `input`, code `schema_invalid`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownName {
    kind: &'static str,
    name: String,
    known: &'static [&'static str],
}

impl UnknownName {
    pub(crate) fn new(kind: &'static str, name: &str, known: &'static [&'static str]) -> Self {
        Self {
            kind,
            name: shorten(name),
            known,
        }
    }

    /// What kind of word was expected, such as "run state".
    #[must_use]
    pub fn kind(&self) -> &'static str {
        self.kind
    }

    /// The names that would have been accepted.
    #[must_use]
    pub fn known(&self) -> &'static [&'static str] {
        self.known
    }
}

impl fmt::Display for UnknownName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "`{}` is not a {}; expected one of: {}",
            self.name,
            self.kind,
            self.known.join(", ")
        )
    }
}

impl std::error::Error for UnknownName {}

/// A name that does not have the shape every registered name must have.
///
/// It maps to the failure class `input`, code `schema_invalid`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidName {
    kind: &'static str,
    name: String,
    longest: usize,
}

impl InvalidName {
    /// What kind of name was expected, such as "mode".
    #[must_use]
    pub fn kind(&self) -> &'static str {
        self.kind
    }
}

impl fmt::Display for InvalidName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "`{}` is not a valid {} name; a name starts with a lowercase letter, continues with \
             lowercase letters, digits, and underscores, and has at most {} characters",
            self.name, self.kind, self.longest
        )
    }
}

impl std::error::Error for InvalidName {}

/// Whether `text` has the shape of a registered name: `[a-z][a-z0-9_]*`, at
/// most `longest` characters.
pub(crate) const fn is_name(text: &str, longest: usize) -> bool {
    let bytes = text.as_bytes();
    if bytes.is_empty() || bytes.len() > longest || !bytes[0].is_ascii_lowercase() {
        return false;
    }
    let mut i = 1;
    while i < bytes.len() {
        let b = bytes[i];
        if !(b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_') {
            return false;
        }
        i += 1;
    }
    true
}

/// Checks a name's shape, and says what is wrong when it is not a name.
pub(crate) fn check_name(
    kind: &'static str,
    text: &str,
    longest: usize,
) -> Result<(), InvalidName> {
    if is_name(text, longest) {
        Ok(())
    } else {
        Err(InvalidName {
            kind,
            name: shorten(text),
            longest,
        })
    }
}

/// At most `SHOWN` characters of `text`, with a mark when it was cut.
fn shorten(text: &str) -> String {
    if text.chars().count() <= SHOWN {
        text.to_owned()
    } else {
        let mut cut: String = text.chars().take(SHOWN).collect();
        cut.push('…');
        cut
    }
}

#[cfg(test)]
mod tests {
    use super::{check_name, is_name, shorten};

    #[test]
    fn a_name_is_lowercase_letters_digits_and_underscores() {
        for good in ["ui", "page_http", "a", "v2", "needs_login"] {
            assert!(is_name(good, 32), "{good}");
        }
        for bad in [
            "",
            "UI",
            "2fa",
            "_x",
            "page-http",
            "page http",
            "naïve",
            "a.b",
        ] {
            assert!(!is_name(bad, 32), "{bad}");
        }
    }

    #[test]
    fn a_name_has_a_longest_length() {
        assert!(is_name(&"a".repeat(32), 32));
        assert!(!is_name(&"a".repeat(33), 32));
    }

    #[test]
    fn a_wrong_name_is_reported_with_the_rule() {
        let error = check_name("mode", "Page-HTTP", 32).unwrap_err();
        let text = error.to_string();
        assert!(text.contains("`Page-HTTP`"), "{text}");
        assert!(text.contains("mode name"), "{text}");
        assert!(text.contains("at most 32"), "{text}");
    }

    #[test]
    fn a_very_long_name_is_cut_before_it_is_shown() {
        let long = "x".repeat(10_000);
        let shown = shorten(&long);
        assert_eq!(shown.chars().count(), 65);
        assert!(shown.ends_with('…'));
        assert!(check_name("mode", &long, 32).unwrap_err().to_string().len() < 400);
    }
}
