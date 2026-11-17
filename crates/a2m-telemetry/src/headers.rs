//! Which headers of an exchange may be recorded. The list is written once,
//! here, for every pack that records exchanges (spec section 217, "Keeping
//! secrets out", rule 2).
//!
//! A header on the list is kept with its value, redacted. Every other header
//! is recorded as present, with its value masked, whatever the capture
//! level. Cookies and authorization are not on the list and never will be.

use a2m_core::Redact;
use serde::{Deserialize, Serialize};

use crate::text::{LABEL_LIMIT, clean_text, limit_text};

/// The headers whose values may be recorded, in lowercase. None of them
/// carries a credential, a session, or an address that could hold a token.
pub const RECORDED_HEADERS: &[&str] = &[
    "accept",
    "accept-encoding",
    "accept-language",
    "allow",
    "cache-control",
    "content-encoding",
    "content-language",
    "content-length",
    "content-type",
    "date",
    "etag",
    "expires",
    "if-match",
    "if-modified-since",
    "if-none-match",
    "if-unmodified-since",
    "last-modified",
    "retry-after",
    "user-agent",
    "vary",
];

/// The most characters of a header's value that are kept.
pub const HEADER_VALUE_LIMIT: usize = 256;

/// One header as an exchange records it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordedHeader {
    /// The header's name, in lowercase.
    pub name: String,
    /// The header's value, when the header is on the list. `None` says the
    /// header was present and its value was masked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

/// Whether a header's value may be recorded. Names are compared without
/// regard to case.
#[must_use]
pub fn may_record_header(name: &str) -> bool {
    RECORDED_HEADERS
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(name))
}

/// One header as it may be recorded: with its redacted value when it is on
/// the list, and as present and masked when it is not.
#[must_use]
pub fn record_header(name: &str, value: &str, redact: &dyn Redact) -> RecordedHeader {
    let safe = |text: &str| redact.redact(&clean_text(&redact.redact(text)));
    RecordedHeader {
        name: limit_text(&safe(&name.to_ascii_lowercase()), LABEL_LIMIT),
        value: may_record_header(name).then(|| limit_text(&safe(value), HEADER_VALUE_LIMIT)),
    }
}

/// Every header of a request or a response, each as it may be recorded, in
/// the order given.
#[must_use]
pub fn record_headers<'a>(
    headers: impl IntoIterator<Item = (&'a str, &'a str)>,
    redact: &dyn Redact,
) -> Vec<RecordedHeader> {
    headers
        .into_iter()
        .map(|(name, value)| record_header(name, value, redact))
        .collect()
}

#[cfg(test)]
mod tests {
    use a2m_core::NoSecrets;

    use super::{RECORDED_HEADERS, RecordedHeader, may_record_header, record_headers};
    use crate::testing::Planted;

    #[test]
    fn cookies_and_authorization_are_never_on_the_list() {
        for never in [
            "cookie",
            "set-cookie",
            "authorization",
            "proxy-authorization",
            "www-authenticate",
            "location",
            "referer",
        ] {
            assert!(!may_record_header(never), "{never}");
            assert!(!may_record_header(&never.to_uppercase()), "{never}");
        }
        for name in RECORDED_HEADERS {
            assert_eq!(*name, name.to_lowercase());
            assert!(!name.contains("cookie") && !name.contains("auth"), "{name}");
        }
    }

    #[test]
    fn a_header_off_the_list_is_recorded_as_present_and_masked() {
        let recorded = record_headers(
            [
                ("Content-Type", "application/json"),
                ("Cookie", "sid=abc123"),
                ("Authorization", "Bearer abc123"),
                ("X-Custom", "anything"),
            ],
            &NoSecrets,
        );
        let masked = |name: &str| RecordedHeader {
            name: name.to_owned(),
            value: None,
        };
        assert_eq!(
            recorded,
            [
                RecordedHeader {
                    name: "content-type".to_owned(),
                    value: Some("application/json".to_owned()),
                },
                masked("cookie"),
                masked("authorization"),
                masked("x-custom"),
            ]
        );
        let json = serde_json::to_string(&recorded).unwrap();
        assert!(
            !json.contains("abc123") && !json.contains("anything"),
            "{json}"
        );
    }

    #[test]
    fn a_kept_value_is_redacted() {
        let redact = Planted::secrets(&["hunter2-token"]);
        let recorded = record_headers([("ETag", "\"hunter2-token\"")], &redact);
        assert_eq!(recorded[0].value.as_deref(), Some("\"[redacted]\""));
    }
}
