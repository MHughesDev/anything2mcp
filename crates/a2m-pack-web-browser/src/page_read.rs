//! A read made from inside the page, with the page's own session: the path
//! it may go to, and what comes back (decision 0190).
//!
//! The rules are those of decision 0144 for a request made from inside a
//! page. The origin is the page's own, and the page must be on one of the
//! tab's origins; the method is GET, and the function that sends it takes
//! no method; the path is made from a pattern and values each of which is
//! one plain segment, refused and never encoded; no header a caller chose
//! and no body is sent; no redirect is followed; the answer is read as
//! text, up to a limit. Only an observing tab makes one, so the tab's own
//! guard, which fails every request of the page that could write, is in
//! force beneath it.

use std::collections::BTreeMap;
use std::fmt;

use a2m_core::Failure;

use crate::error::{codes, input};

/// The longest value one placeholder may hold, in bytes.
pub const MAX_SEGMENT_BYTES: usize = 512;

/// Whether a text is one plain segment of an address: letters and digits
/// of ASCII, `-`, `.`, `_`, and `~`, not empty, not made of dots only, and
/// at most [`MAX_SEGMENT_BYTES`]. Any other text could leave its segment in
/// some application: a `/`, a `\`, a `?`, a `#`, a `%`, a `:`, a space, or
/// a control character (decision 0187).
#[must_use]
pub fn plain_segment(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= MAX_SEGMENT_BYTES
        && !text.chars().all(|c| c == '.')
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '~'))
}

/// The path of a read made from inside the page: a pattern that starts
/// with `/`, whose every segment is plain text or one whole placeholder,
/// `{name}`, each filled with a value that is one plain segment. A query, a
/// fragment, and an empty segment cannot be written in it. The origin is
/// not part of it: the read goes to the page's own.
#[derive(Clone, PartialEq, Eq)]
pub struct PagePath(String);

impl PagePath {
    /// Makes the path from its pattern and the values of its placeholders,
    /// by name. A value that is not one plain segment is refused, never
    /// encoded.
    ///
    /// # Errors
    ///
    /// `input`, `address_invalid` for a pattern that is not one;
    /// `input`, `address_value_refused` for a placeholder with no value,
    /// or a value that is not one plain segment. The message names the
    /// placeholder and never holds the value.
    pub fn fill(pattern: &str, values: &BTreeMap<String, String>) -> Result<Self, Failure> {
        let unusable = || {
            input(
                codes::ADDRESS_INVALID,
                "The path of a read from inside the page starts with `/`, and each of its segments is plain text or one whole placeholder such as `{order_id}`.",
                "Correct the path in the target's files.",
            )
        };
        let Some(rest) = pattern.strip_prefix('/') else {
            return Err(unusable());
        };
        let mut path = String::new();
        if rest.is_empty() {
            path.push('/');
        }
        for part in rest.split('/').filter(|_| !rest.is_empty()) {
            path.push('/');
            let hole = part
                .strip_prefix('{')
                .and_then(|inner| inner.strip_suffix('}'));
            match hole {
                Some(name) if plain_segment(name) => {
                    let refused = |why: &str| {
                        input(
                            codes::ADDRESS_VALUE_REFUSED,
                            format!("The value for `{name}` cannot stand in the path: {why}."),
                            "Give a value that is one segment of an address: letters, digits, `-`, `.`, `_`, and `~`.",
                        )
                    };
                    let value = values
                        .get(name)
                        .ok_or_else(|| refused("it was not given"))?;
                    if !plain_segment(value) {
                        return Err(refused(
                            "it is empty, made of dots only, longer than 512 bytes, or holds a character that is not a letter, a digit, `-`, `.`, `_`, or `~`",
                        ));
                    }
                    path.push_str(value);
                }
                Some(_) => return Err(unusable()),
                None if plain_segment(part) => path.push_str(part),
                None => return Err(unusable()),
            }
        }
        Ok(Self(path))
    }

    /// The path, as it is sent.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for PagePath {
    /// A path can hold a record's key; its debug form gives its length.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PagePath")
            .field("characters", &self.0.chars().count())
            .finish()
    }
}

/// What a read made from inside the page answered: the status, the media
/// type, and the body as text, with every known secret replaced. No header
/// is given: a cookie or a token a page set travels in one, and only the
/// media type is needed to read the body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageAnswer {
    /// The status.
    pub status: u16,
    /// The media type, in lowercase, without its parameters; empty when
    /// the answer states none.
    pub media_type: String,
    /// The body.
    pub body: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect()
    }

    #[test]
    fn a_path_is_its_pattern_with_one_plain_segment_for_each_placeholder() {
        let made = PagePath::fill(
            "/api/orders/{order_id}/totals",
            &values(&[("order_id", "A-100")]),
        )
        .unwrap();
        assert_eq!(made.as_str(), "/api/orders/A-100/totals");
        assert_eq!(PagePath::fill("/", &values(&[])).unwrap().as_str(), "/");
        // Its debug form holds no key.
        assert!(!format!("{made:?}").contains("A-100"));

        for value in [
            "",
            ".",
            "..",
            "a/b",
            "a\\b",
            "a?b",
            "a#b",
            "%2e%2e",
            "a b",
            "a:b",
            "a\nb",
            "é",
            &"x".repeat(513),
        ] {
            let refused = PagePath::fill("/api/orders/{order_id}", &values(&[("order_id", value)]))
                .unwrap_err();
            assert_eq!(refused.code(), &codes::ADDRESS_VALUE_REFUSED, "{value:?}");
            if value.len() > 2 {
                assert!(!refused.message().contains(value), "{value:?}");
            }
        }
        let missing = PagePath::fill("/api/orders/{order_id}", &values(&[])).unwrap_err();
        assert_eq!(missing.code(), &codes::ADDRESS_VALUE_REFUSED);
        for pattern in [
            "api/orders",
            "//other.example/x",
            "/a//b",
            "/a?b=1",
            "/a#b",
            "/{a b}",
            "/x{order_id}",
            "/a/",
        ] {
            let refused = PagePath::fill(pattern, &values(&[("order_id", "A-1")])).unwrap_err();
            assert_eq!(refused.code(), &codes::ADDRESS_INVALID, "{pattern}");
        }
    }
}
