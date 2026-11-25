//! A read made from inside the page by a method that may write, which a
//! person asserted changes nothing: the request an observing tab sends for a
//! read binding that declares a body (decision 0999, after decision 0392,
//! point 6).
//!
//! It is the read of [`crate::PagePath`] with a method and a body beside
//! it, and nothing more. The method is one of a closed list. The body is a
//! JSON object or a form, or none, whose field names are plain names and
//! whose values are text, whole numbers, or truth values, under the same
//! bounds as a write's. No header a caller chose is sent, no token is read
//! from the page, no redirect is followed, and the answer is read as text up
//! to the tab's limit.
//!
//! What lets it leave is not this module: only [`crate::ObserveTab`] has the
//! call, and that tab's own guard fails every request of its page that could
//! write but the asserted reads it was opened with, matched by method,
//! origin, path, and body. A request this module builds goes through that
//! guard as any request of the page does, so a tab opened with no asserted
//! read sends none.

use std::collections::BTreeMap;
use std::fmt;

use a2m_core::Failure;
use serde::Serialize;

use crate::error::{codes, input};
use crate::page_read::PagePath;
use crate::page_write::{
    BodyEncoding, FieldValue, LONGEST_FIELD_TEXT, MOST_FIELDS, WriteMethod, plain_write_name,
};

/// One read made from inside the page by a method other than GET. Made only
/// through [`PageAsk::new`], which holds every name to [`plain_write_name`]
/// and every value to the bounds of a write.
#[derive(Clone, PartialEq, Eq)]
pub struct PageAsk {
    method: WriteMethod,
    path: PagePath,
    body: Option<(BodyEncoding, BTreeMap<String, FieldValue>)>,
}

impl fmt::Debug for PageAsk {
    /// The body's values can be a record's business data; only its field
    /// names are shown.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PageAsk")
            .field("method", &self.method)
            .field("path", &self.path)
            .field(
                "fields",
                &self
                    .body
                    .as_ref()
                    .map(|(_, fields)| fields.keys().collect::<Vec<_>>()),
            )
            .finish()
    }
}

impl PageAsk {
    /// The read, checked. `body` is `None` for a request that carries none.
    ///
    /// # Errors
    ///
    /// `input`, `address_invalid` for a field whose name is not a plain
    /// name, for more than [`MOST_FIELDS`] fields, and for a text longer
    /// than [`LONGEST_FIELD_TEXT`]. Nothing is sent.
    pub fn new(
        method: WriteMethod,
        path: PagePath,
        body: Option<(BodyEncoding, BTreeMap<String, FieldValue>)>,
    ) -> Result<Self, Failure> {
        let refused = |why: &str| {
            input(
                codes::ADDRESS_INVALID,
                format!("A read from inside the page cannot be made: {why}."),
                "Correct the entry's names: each is a letter, then letters, digits, `-`, and `_`.",
            )
        };
        if let Some((_, fields)) = &body {
            if fields.len() > MOST_FIELDS {
                return Err(refused("its body holds more fields than a request may"));
            }
            for (name, value) in fields {
                if !plain_write_name(name) {
                    return Err(refused("a field of its body has a name that is not plain"));
                }
                if let FieldValue::Text(text) = value
                    && text.chars().count() > LONGEST_FIELD_TEXT
                {
                    return Err(refused(
                        "a field of its body is longer than a request may send",
                    ));
                }
            }
        }
        Ok(Self { method, path, body })
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

    /// What the helper in the page is given: the method, the path, how the
    /// body is written, and its fields.
    pub(crate) fn spec(&self) -> AskSpec<'_> {
        let (encoding, fields) = match &self.body {
            None => ("none", BTreeMap::new()),
            Some((encoding, fields)) => (
                match encoding {
                    BodyEncoding::Json => "json",
                    BodyEncoding::Form => "form",
                },
                fields
                    .iter()
                    .map(|(name, value)| {
                        let value = match value {
                            FieldValue::Text(text) => serde_json::Value::String(text.clone()),
                            FieldValue::Whole(number) => serde_json::Value::from(*number),
                            FieldValue::Truth(flag) => serde_json::Value::Bool(*flag),
                        };
                        (name.as_str(), value)
                    })
                    .collect(),
            ),
        };
        AskSpec {
            method: self.method.as_str(),
            path: self.path.as_str(),
            encoding,
            fields,
        }
    }
}

#[derive(Serialize)]
pub(crate) struct AskSpec<'a> {
    method: &'static str,
    path: &'a str,
    encoding: &'static str,
    fields: BTreeMap<&'a str, serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path() -> PagePath {
        PagePath::fill("/api/cases/rows/count", &BTreeMap::new()).unwrap()
    }

    #[test]
    fn a_read_by_another_method_names_only_plain_fields_and_shows_no_value() {
        let fields = |pairs: &[(&str, FieldValue)]| {
            pairs
                .iter()
                .map(|(name, value)| ((*name).to_owned(), value.clone()))
                .collect::<BTreeMap<_, _>>()
        };
        let asked = PageAsk::new(
            WriteMethod::Post,
            path(),
            Some((
                BodyEncoding::Json,
                fields(&[
                    ("status", FieldValue::Text("open-secret-value".to_owned())),
                    ("op", FieldValue::Text("count".to_owned())),
                ]),
            )),
        )
        .unwrap();
        let spec = serde_json::to_value(asked.spec()).unwrap();
        assert_eq!(spec["method"], "POST");
        assert_eq!(spec["encoding"], "json");
        assert_eq!(spec["fields"]["op"], "count");
        assert!(!format!("{asked:?}").contains("open-secret-value"));

        let none = PageAsk::new(WriteMethod::Post, path(), None).unwrap();
        let spec = serde_json::to_value(none.spec()).unwrap();
        assert_eq!(spec["encoding"], "none");
        assert_eq!(spec["fields"], serde_json::json!({}));

        for name in ["", "1st", "a b", "a\"b", &"a".repeat(65)] {
            let refused = PageAsk::new(
                WriteMethod::Post,
                path(),
                Some((
                    BodyEncoding::Json,
                    fields(&[(name, FieldValue::Truth(true))]),
                )),
            );
            assert!(refused.is_err(), "{name}");
        }
        let long = "x".repeat(LONGEST_FIELD_TEXT + 1);
        assert!(
            PageAsk::new(
                WriteMethod::Post,
                path(),
                Some((BodyEncoding::Form, fields(&[("q", FieldValue::Text(long))]))),
            )
            .is_err()
        );
    }
}
