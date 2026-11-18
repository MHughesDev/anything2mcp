//! A write made from inside the page, with the page's own session and the
//! page's own anti-forgery token: what may be sent, and only from a tab
//! that may act (decision 0206).
//!
//! The rules are those of the read (decision 0190), with what a write needs
//! beside them. The method is one of a closed list. The path is a
//! [`PagePath`], made from a pattern and values each of which is one plain
//! segment. The body is a JSON object or a form, whose field names are
//! plain names and whose values are text, whole numbers, or truth values.
//! No header a caller chose is sent: the one header that may be named is
//! where the token goes, and only a name that is not one the browser or the
//! connection sets. The token is read from the page in the same call that
//! sends the write, by the place the target's entry names, and it never
//! leaves the page: it is not handed back, and every copy of it in the
//! answer is replaced in the page before the answer is returned.
//!
//! Only [`crate::ActTab`] has the call. An observing tab has none, and its
//! browser fails every request of its page whose method could write.

use std::collections::BTreeMap;
use std::fmt;

use a2m_core::Failure;
use serde::Serialize;

use crate::error::{codes, input};
use crate::page_read::PagePath;

/// The longest name of a field, a header, a meta element, an input, or a
/// cookie a write names, in bytes.
pub const LONGEST_WRITE_NAME: usize = 64;

/// The most fields a body may hold.
pub const MOST_FIELDS: usize = 64;

/// The longest text one field's value may hold, in characters.
pub const LONGEST_FIELD_TEXT: usize = 65_536;

/// The headers a write may never name for its token: those the browser or
/// the connection sets, those that carry a session, and the body's own.
pub const REFUSED_HEADERS: &[&str] = &[
    "accept-charset",
    "accept-encoding",
    "access-control-request-headers",
    "access-control-request-method",
    "authorization",
    "connection",
    "content-length",
    "content-type",
    "cookie",
    "cookie2",
    "date",
    "dnt",
    "expect",
    "host",
    "keep-alive",
    "origin",
    "proxy-authorization",
    "referer",
    "set-cookie",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
    "user-agent",
    "via",
];

/// Whether a text is a name a write may use for a field, a header, a meta
/// element, an input, or a cookie: an ASCII letter, then letters, digits,
/// `-`, and `_`, at most [`LONGEST_WRITE_NAME`] bytes. No caller's text is
/// ever such a name; each comes from the target's entry.
#[must_use]
pub fn plain_write_name(name: &str) -> bool {
    name.len() <= LONGEST_WRITE_NAME
        && name.starts_with(|c: char| c.is_ascii_alphabetic())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
}

/// The methods a write may be sent with, a closed list. A read is made
/// with [`crate::ObserveTab::read_from_page`], which sends GET alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum WriteMethod {
    /// `POST`.
    Post,
    /// `PUT`.
    Put,
    /// `PATCH`.
    Patch,
    /// `DELETE`.
    Delete,
}

impl WriteMethod {
    /// The method as it is sent.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
        }
    }

    /// The method a word names, in capitals; `None` for any other word.
    #[must_use]
    pub fn from_word(word: &str) -> Option<Self> {
        Some(match word {
            "POST" => Self::Post,
            "PUT" => Self::Put,
            "PATCH" => Self::Patch,
            "DELETE" => Self::Delete,
            _ => return None,
        })
    }
}

/// Where the page holds its anti-forgery token. Which of them, and its
/// name, is the target's knowledge, written in its entry.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TokenFrom {
    /// The `content` of the `meta` element of this `name`.
    Meta(String),
    /// The value of the hidden `input` of this `name`.
    Input(String),
    /// The cookie of this name, as the page's own script may read it.
    Cookie(String),
}

/// Where the token is sent.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TokenTo {
    /// In the header of this name.
    Header(String),
    /// As the body's field of this name.
    Field(String),
}

/// The page's anti-forgery token: where it is read, and where it is sent.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AntiForgery {
    /// Where the page holds it.
    pub from: TokenFrom,
    /// Where it goes.
    pub to: TokenTo,
}

/// One value of a body field: typed, never a nested value or a number with
/// a fraction, which the page's own number type would round.
#[derive(Clone, PartialEq, Eq)]
pub enum FieldValue {
    /// Text.
    Text(String),
    /// A whole number.
    Whole(i64),
    /// A truth value.
    Truth(bool),
}

impl fmt::Debug for FieldValue {
    /// A value can be a record's business data; its debug form gives its
    /// kind and size only.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text(text) => write!(f, "Text({} characters)", text.chars().count()),
            Self::Whole(_) => f.write_str("Whole"),
            Self::Truth(_) => f.write_str("Truth"),
        }
    }
}

/// How the body is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BodyEncoding {
    /// A JSON object, `application/json`.
    Json,
    /// A form, `application/x-www-form-urlencoded`, every value as text.
    Form,
}

/// One write made from inside the page. Made only through
/// [`PageWrite::new`], which holds every name to [`plain_write_name`] and
/// refuses a header the browser or the connection sets.
#[derive(Clone, PartialEq, Eq)]
pub struct PageWrite {
    method: WriteMethod,
    path: PagePath,
    encoding: BodyEncoding,
    fields: BTreeMap<String, FieldValue>,
    token: Option<AntiForgery>,
}

impl fmt::Debug for PageWrite {
    /// The names of the fields and the length of the path: never a value.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PageWrite")
            .field("method", &self.method)
            .field("path", &self.path)
            .field("encoding", &self.encoding)
            .field("fields", &self.fields.keys().collect::<Vec<_>>())
            .field("token", &self.token)
            .finish()
    }
}

impl PageWrite {
    /// The write, checked.
    ///
    /// # Errors
    ///
    /// `input`, `address_invalid` for a field, a header, or a token's
    /// place whose name is not a plain name, for a header of
    /// [`REFUSED_HEADERS`], for a token sent as a field the body already
    /// holds, for more than [`MOST_FIELDS`] fields, and for a text longer
    /// than [`LONGEST_FIELD_TEXT`]. Nothing is sent.
    pub fn new(
        method: WriteMethod,
        path: PagePath,
        encoding: BodyEncoding,
        fields: BTreeMap<String, FieldValue>,
        token: Option<AntiForgery>,
    ) -> Result<Self, Failure> {
        let refused = |why: &str| {
            input(
                codes::ADDRESS_INVALID,
                format!("A write from inside the page cannot be made: {why}."),
                "Correct the entry's names: each is a letter, then letters, digits, `-`, and `_`.",
            )
        };
        if fields.len() > MOST_FIELDS {
            return Err(refused("its body holds more fields than a write may"));
        }
        for (name, value) in &fields {
            if !plain_write_name(name) {
                return Err(refused("a field of its body has a name that is not plain"));
            }
            if let FieldValue::Text(text) = value
                && text.chars().count() > LONGEST_FIELD_TEXT
            {
                return Err(refused(
                    "a field of its body is longer than a write may send",
                ));
            }
        }
        if let Some(token) = &token {
            let (TokenFrom::Meta(from) | TokenFrom::Input(from) | TokenFrom::Cookie(from)) =
                &token.from;
            if !plain_write_name(from) {
                return Err(refused(
                    "where its token is read has a name that is not plain",
                ));
            }
            match &token.to {
                TokenTo::Header(name) => {
                    let lower = name.to_ascii_lowercase();
                    if !plain_write_name(name)
                        || REFUSED_HEADERS.contains(&lower.as_str())
                        || lower.starts_with("sec-")
                        || lower.starts_with("proxy-")
                    {
                        return Err(refused(
                            "its token goes in a header that is not plain, or that the browser or the connection sets",
                        ));
                    }
                }
                TokenTo::Field(name) => {
                    if !plain_write_name(name) || fields.contains_key(name) {
                        return Err(refused(
                            "its token goes in a field whose name is not plain, or that the body already holds",
                        ));
                    }
                }
            }
        }
        Ok(Self {
            method,
            path,
            encoding,
            fields,
            token,
        })
    }

    /// The method.
    #[must_use]
    pub const fn method(&self) -> WriteMethod {
        self.method
    }

    /// The path.
    #[must_use]
    pub const fn path(&self) -> &PagePath {
        &self.path
    }

    /// What the helper in the page is given: names, the method, the path,
    /// and the values, and never a token.
    pub(crate) fn spec(&self) -> Spec<'_> {
        let fields = self
            .fields
            .iter()
            .map(|(name, value)| {
                let value = match value {
                    FieldValue::Text(text) => serde_json::Value::String(text.clone()),
                    FieldValue::Whole(number) => serde_json::Value::from(*number),
                    FieldValue::Truth(flag) => serde_json::Value::Bool(*flag),
                };
                (name.as_str(), value)
            })
            .collect();
        let token = self.token.as_ref().map(|token| {
            let (from, name) = match &token.from {
                TokenFrom::Meta(name) => ("meta", name.as_str()),
                TokenFrom::Input(name) => ("input", name.as_str()),
                TokenFrom::Cookie(name) => ("cookie", name.as_str()),
            };
            let (to, to_name) = match &token.to {
                TokenTo::Header(name) => ("header", name.as_str()),
                TokenTo::Field(name) => ("field", name.as_str()),
            };
            TokenSpec {
                from,
                name,
                to,
                to_name,
            }
        });
        Spec {
            method: self.method.as_str(),
            path: self.path.as_str(),
            encoding: match self.encoding {
                BodyEncoding::Json => "json",
                BodyEncoding::Form => "form",
            },
            fields,
            token,
        }
    }
}

#[derive(Serialize)]
pub(crate) struct TokenSpec<'a> {
    from: &'static str,
    name: &'a str,
    to: &'static str,
    to_name: &'a str,
}

#[derive(Serialize)]
pub(crate) struct Spec<'a> {
    method: &'static str,
    path: &'a str,
    encoding: &'static str,
    fields: BTreeMap<&'a str, serde_json::Value>,
    token: Option<TokenSpec<'a>>,
}

/// Why the page's anti-forgery token could not be sent. In each case
/// nothing was sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenProblem {
    /// The page holds no token where the entry says.
    Missing,
    /// The page holds two different tokens where the entry says.
    Several,
    /// The text the page holds there is not of a form a token is sent in:
    /// 8 to 512 characters of ASCII letters, digits, `-`, `_`, `.`, `~`,
    /// `+`, `/`, and `=`.
    Unusable,
}

impl fmt::Display for TokenProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Missing => "the page holds no anti-forgery token where the entry says",
            Self::Several => "the page holds more than one anti-forgery token where the entry says",
            Self::Unusable => {
                "what the page holds where the entry says is not of a form an anti-forgery token is sent in"
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path() -> PagePath {
        PagePath::fill("/api/orders/A-100/note", &BTreeMap::new()).unwrap()
    }

    fn write(
        fields: &[(&str, FieldValue)],
        token: Option<AntiForgery>,
    ) -> Result<PageWrite, Failure> {
        PageWrite::new(
            WriteMethod::Put,
            path(),
            BodyEncoding::Json,
            fields
                .iter()
                .map(|(name, value)| ((*name).to_owned(), value.clone()))
                .collect(),
            token,
        )
    }

    fn token(to: TokenTo) -> Option<AntiForgery> {
        Some(AntiForgery {
            from: TokenFrom::Meta("csrf-token".to_owned()),
            to,
        })
    }

    #[test]
    fn a_write_names_only_plain_names_and_no_header_the_browser_sets() {
        let note = ("note", FieldValue::Text("Side door".to_owned()));
        let made = write(
            &[note.clone()],
            token(TokenTo::Header("X-CSRF-Token".to_owned())),
        )
        .unwrap();
        // Its debug form holds no value.
        assert!(!format!("{made:?}").contains("Side door"));
        assert_eq!(made.method().as_str(), "PUT");

        for header in [
            "Cookie",
            "authorization",
            "Content-Type",
            "Origin",
            "Sec-Fetch-Site",
            "X Token",
            "x:y",
            "",
        ] {
            let refused =
                write(&[note.clone()], token(TokenTo::Header(header.to_owned()))).unwrap_err();
            assert_eq!(refused.code(), &codes::ADDRESS_INVALID, "{header}");
        }
        for field in ["note", "a b", "a\"b", "1st"] {
            let refused =
                write(&[note.clone()], token(TokenTo::Field(field.to_owned()))).unwrap_err();
            assert_eq!(refused.code(), &codes::ADDRESS_INVALID, "{field}");
        }
        for name in ["no te", "note\"", "", "note]"] {
            let refused = write(&[(name, FieldValue::Truth(true))], None).unwrap_err();
            assert_eq!(refused.code(), &codes::ADDRESS_INVALID, "{name}");
        }
        let from_odd = PageWrite::new(
            WriteMethod::Put,
            path(),
            BodyEncoding::Form,
            BTreeMap::new(),
            Some(AntiForgery {
                from: TokenFrom::Cookie("a;b".to_owned()),
                to: TokenTo::Header("X-CSRF-Token".to_owned()),
            }),
        )
        .unwrap_err();
        assert_eq!(from_odd.code(), &codes::ADDRESS_INVALID);
        let long = write(
            &[("note", FieldValue::Text("x".repeat(LONGEST_FIELD_TEXT + 1)))],
            None,
        )
        .unwrap_err();
        assert_eq!(long.code(), &codes::ADDRESS_INVALID);

        // The closed list of methods; GET is not one of them.
        for word in ["POST", "PUT", "PATCH", "DELETE"] {
            assert_eq!(WriteMethod::from_word(word).unwrap().as_str(), word);
        }
        for word in ["GET", "put", "HEAD", "OPTIONS", "TRACE", "CONNECT"] {
            assert!(WriteMethod::from_word(word).is_none(), "{word}");
        }
    }
}
