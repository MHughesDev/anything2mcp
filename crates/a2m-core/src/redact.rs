//! Redaction and keyed digests: the two interfaces the vault implements and
//! everything that emits text or names a stored thing takes.

use sha2::{Digest as _, Sha256};

use crate::digest::Digest;
use crate::failure::Failure;
use crate::ident::WorkspaceId;
use crate::value::{Value, Values};
use crate::vocabulary::vocabulary;

/// What stands in for a secret wherever one was found.
pub const REDACTED: &str = "[redacted]";

/// Removes every registered secret from text on its way out.
///
/// The vault implements this. Everything that emits text takes one: a tool
/// result, a log line, a trace, a capture, an error message. The helpers
/// below are the same for every implementation, so a caller redacts a whole
/// failure or a whole set of values in one call and cannot miss a part.
pub trait Redact: Send + Sync {
    /// Returns `text` with every registered secret replaced by
    /// [`REDACTED`]. Never fails, and never returns the input unchanged when
    /// it contains a registered secret.
    fn redact(&self, text: &str) -> String;

    /// True when `text` holds a registered secret. This is the check on the
    /// way out. An implementation may answer it more cheaply than by
    /// redacting; the answer must be the same.
    fn contains_secret(&self, text: &str) -> bool {
        self.redact(text) != text
    }

    /// The failure with its sentence, its next step, and what the system
    /// said redacted. The class and the code are names and are kept.
    fn redact_failure(&self, failure: &Failure) -> Failure {
        let clean = Failure::new(
            failure.class(),
            failure.code().clone(),
            self.redact(failure.message()),
            self.redact(failure.next_step()),
        );
        match failure.said() {
            Some(said) => clean.with_said(self.redact(said)),
            None => clean,
        }
    }

    /// The value with every secret in it redacted, at any depth. A field's
    /// name is redacted too, because a system chooses the names it returns.
    ///
    /// A value that is not text is judged by the text it is written as: a
    /// whole number and a length of time by their digits, a decimal by its
    /// shortest exact form, a date as `2026-11-17` and as `20261117`, a
    /// moment by its milliseconds. One that holds a registered secret is
    /// replaced whole by the text [`REDACTED`], so a secret that is a
    /// number, such as a PIN, does not pass as a number. The cost is that a
    /// short numeric secret also removes every number whose digits hold it,
    /// and that the value's kind becomes text. A secret written with leading
    /// zeros, or with trailing zeros after a decimal mark, is not found in a
    /// number that has lost them.
    fn redact_value(&self, value: &Value) -> Value {
        let holds = |text: &str| self.contains_secret(text);
        let secret = match value {
            Value::Text(text) => return Value::Text(self.redact(text)),
            Value::List(items) => {
                return Value::List(items.iter().map(|item| self.redact_value(item)).collect());
            }
            Value::Record(values) => return Value::Record(self.redact_values(values)),
            Value::Null | Value::Bool(_) => false,
            Value::Int(number) => holds(&number.to_string()),
            Value::Decimal(number) => holds(&number.to_string()),
            Value::Duration(length) => holds(&length.get().to_string()),
            Value::Instant(moment) => holds(&moment.as_unix_millis().to_string()),
            Value::Date(date) => {
                let written = date.to_string();
                holds(&written) || holds(&written.replace('-', ""))
            }
        };
        if secret {
            Value::Text(REDACTED.to_owned())
        } else {
            value.clone()
        }
    }

    /// The named values with every secret in them redacted. Two names that
    /// redact to the same text are both kept: the later one, in order of
    /// name, has a number added, so no value takes another's place.
    fn redact_values(&self, values: &Values) -> Values {
        let mut clean = Values::new();
        for (name, value) in values.iter() {
            let redacted = self.redact(name);
            let mut unique = redacted.clone();
            let mut count = 1u32;
            while clean.contains(&unique) {
                count += 1;
                unique = format!("{redacted} ({count})");
            }
            clean.insert(unique, self.redact_value(value));
        }
        clean
    }
}

/// A redactor with no secret registered. It changes nothing, and says so by
/// its name.
///
/// For tests, and for code that runs where no vault can exist. A process
/// that holds a vault never uses it: the vault's redactor is the one that
/// knows the secrets.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoSecrets;

impl Redact for NoSecrets {
    fn redact(&self, text: &str) -> String {
        text.to_owned()
    }

    fn contains_secret(&self, _text: &str) -> bool {
        false
    }
}

vocabulary! {
    /// What a keyed digest is used for. The purpose is part of what is
    /// digested, so the same bytes under two purposes never give the same
    /// digest, and one use can never be matched against another.
    pub enum DigestPurpose: "digest purpose" {
        /// Naming a stored blob by its content.
        BlobName => "blob_name",
        /// Standing in for a restricted value in a trace or a diff.
        RestrictedValue => "restricted_value",
        /// Naming a record's key in the ledger and in a lock.
        RecordKey => "record_key",
        /// Matching a caller's idempotency key without keeping it.
        CallerKey => "caller_key",
    }
}

/// Names a thing without revealing it.
///
/// A keyed digest lets equal inputs be matched, while someone who can read
/// the digest and guess the input cannot confirm the guess without the key.
/// The vault implements this, and holds the key.
///
/// The key is separate for each workspace, so the same content in two
/// workspaces has two names and nothing can be matched across the boundary
/// (decision 0069).
pub trait KeyedDigest: Send + Sync {
    /// The digest of `bytes` for `purpose`, under the key of `workspace`.
    /// An implementation computes it with [`keyed_digest_under`], so that
    /// every implementation gives the same answer for the same key.
    fn keyed_digest(&self, workspace: &WorkspaceId, purpose: DigestPurpose, bytes: &[u8])
    -> Digest;
}

/// The one construction of a keyed digest. `key` is the installation's
/// digest key, which the vault holds.
///
/// A key for the workspace is derived first, as the HMAC-SHA-256 of the
/// workspace's identifier under `key`. The digest is then the HMAC-SHA-256,
/// under the workspace's key, of the purpose's name, one zero byte, and the
/// bytes.
#[must_use]
pub fn keyed_digest_under(
    key: &[u8],
    workspace: &WorkspaceId,
    purpose: DigestPurpose,
    bytes: &[u8],
) -> Digest {
    let workspace_key = hmac_sha256(
        key,
        &[b"anything2mcp workspace digest key 1", workspace.as_bytes()],
    );
    hmac_sha256(
        workspace_key.as_bytes(),
        &[purpose.as_str().as_bytes(), &[0], bytes],
    )
}

/// HMAC-SHA-256 of the parts, one after another, under `key` (RFC 2104).
#[must_use]
pub fn hmac_sha256(key: &[u8], parts: &[&[u8]]) -> Digest {
    const BLOCK: usize = 64;
    let mut block = [0u8; BLOCK];
    if key.len() > BLOCK {
        block[..Digest::LENGTH].copy_from_slice(Digest::of(key).as_bytes());
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    inner.update(block.map(|byte| byte ^ 0x36));
    for part in parts {
        inner.update(part);
    }
    let mut outer = Sha256::new();
    outer.update(block.map(|byte| byte ^ 0x5c));
    outer.update(inner.finalize());
    let mut out = [0u8; Digest::LENGTH];
    out.copy_from_slice(&outer.finalize());
    Digest::from_bytes(out)
}

#[cfg(test)]
mod tests {
    use super::{
        DigestPurpose, KeyedDigest, NoSecrets, REDACTED, Redact, hmac_sha256, keyed_digest_under,
    };
    use crate::{
        Date, Decimal, Digest, Failure, FailureClass, FailureCode, Millis, Timestamp, Value,
        Values, WorkspaceId,
    };

    const SECRET: &str = "correct horse battery staple";

    /// A redactor that knows one secret and implements only `redact`, so the
    /// helpers under test are the trait's own.
    struct Knows(&'static str);

    impl Redact for Knows {
        fn redact(&self, text: &str) -> String {
            text.replace(self.0, REDACTED)
        }
    }

    #[test]
    fn the_check_on_the_way_out_agrees_with_redaction() {
        let redactor = Knows(SECRET);
        assert!(redactor.contains_secret(&format!("token={SECRET};")));
        assert!(!redactor.contains_secret("token=other;"));
        assert!(!redactor.contains_secret(""));
        assert_eq!(
            redactor.redact(&format!("a {SECRET} b {SECRET}")),
            "a [redacted] b [redacted]"
        );
    }

    #[test]
    fn a_failure_is_redacted_in_its_sentence_and_its_next_step_and_keeps_its_names() {
        let failure = Failure::new(
            FailureClass::Rejected,
            FailureCode::new("system_refused").unwrap(),
            format!("The system answered: bad token {SECRET}."),
            format!("Sign in again; do not send {SECRET}."),
        );
        let clean = Knows(SECRET).redact_failure(&failure);
        assert_eq!(clean.class(), FailureClass::Rejected);
        assert_eq!(clean.code().as_str(), "system_refused");
        assert_eq!(
            clean.message(),
            "The system answered: bad token [redacted]."
        );
        assert_eq!(clean.next_step(), "Sign in again; do not send [redacted].");
        assert!(!format!("{clean:?}").contains(SECRET));
    }

    #[test]
    fn what_the_system_said_is_kept_apart_bounded_and_redacted() {
        let failure = Failure::new(
            FailureClass::Rejected,
            FailureCode::new("system_refused").unwrap(),
            "The system refused the record.",
            "Correct the record and call again.",
        )
        .with_said(format!("total must be positive\n\u{1b}[31m {SECRET}"));
        // Never joined into the product's own words, and kept whole: line
        // breaks and control characters as the system gave them.
        assert_eq!(failure.message(), "The system refused the record.");
        assert_eq!(
            failure.said(),
            Some(format!("total must be positive\n\u{1b}[31m {SECRET}").as_str())
        );
        let clean = Knows(SECRET).redact_failure(&failure);
        assert_eq!(
            clean.said(),
            Some("total must be positive\n\u{1b}[31m [redacted]")
        );
        // A door shows it escaped, never flattened.
        assert_eq!(
            crate::said_shown(clean.said().unwrap(), crate::SAID_LONGEST),
            (
                "total must be positive\\n\\u{1b}[31m [redacted]".to_owned(),
                false
            )
        );
        // Above the ceiling it is dropped whole, never cut to a prefix.
        let long = failure
            .clone()
            .with_said("x".repeat(crate::SAID_CEILING + 1));
        assert_eq!(long.said(), Some(crate::SAID_TOO_LONG));
        let whole = failure.clone().with_said("x".repeat(5_000));
        assert_eq!(whole.said().unwrap().len(), 5_000);
        // Nothing to keep is nothing.
        assert_eq!(failure.clone().with_said(" \n\t ").said(), None);
        // A failure that stands in for another carries its words.
        let standing = Failure::new(
            FailureClass::PossibleCommit,
            FailureCode::new("no_confirmation").unwrap(),
            "In doubt.",
            "Reconcile.",
        )
        .carrying_said(&clean);
        assert_eq!(standing.said(), clean.said());
    }

    /// Decision 0185: nothing is cut, joined, or flattened before the
    /// redactor sees what the system said.
    #[test]
    fn a_secret_of_several_lines_or_across_the_display_cut_is_redacted_whole() {
        const KEY: &str = "secret-for-tests-line-one\nsecret-for-tests-line-two\n";
        let failure = Failure::new(
            FailureClass::Rejected,
            FailureCode::new("system_refused").unwrap(),
            "The system refused the record.",
            "Correct the record and call again.",
        );
        // A secret of several lines, echoed in an error.
        let echoed = failure
            .clone()
            .with_said(format!("could not parse key:\n{KEY}end"));
        let clean = Knows(KEY).redact_failure(&echoed);
        let said = clean.said().unwrap();
        assert!(
            !said.contains("line-one") && !said.contains("line-two"),
            "{said}"
        );
        assert_eq!(said, "could not parse key:\n[redacted]end");

        // A secret that starts near the old cut of 400 characters.
        let before = "x".repeat(crate::SAID_LONGEST - 20);
        let straddles = failure.with_said(format!("{before}{SECRET} and more"));
        let clean = Knows(SECRET).redact_failure(&straddles);
        let said = clean.said().unwrap();
        assert!(!said.contains("battery"), "the secret's tail passed");
        assert!(!said.contains("correct horse"), "the secret's head passed");
        // Cut for display after redaction, the mark is never split: what is
        // shown ends before it.
        let (shown, cut) = crate::said_shown(said, crate::SAID_LONGEST - 10);
        assert!(cut);
        assert_eq!(shown, format!("{before}[redacted]…"));
        let (shown, cut) = crate::said_shown(said, crate::SAID_LONGEST - 15);
        assert!(cut);
        assert_eq!(shown, format!("{before}…"));
    }

    #[test]
    fn values_are_redacted_at_every_depth_and_in_their_names() {
        let values = Values::new()
            .with("note", format!("see {SECRET}"))
            .with("count", 3i64)
            .with(
                "lines",
                vec![
                    Value::from(SECRET),
                    Value::Record(Values::new().with(SECRET, SECRET)),
                ],
            );
        let clean = Knows(SECRET).redact_values(&values);
        assert!(!format!("{clean:?}").contains(SECRET));
        assert!(
            !Knows(SECRET).contains_secret(&String::from_utf8(clean.canonical_bytes()).unwrap())
        );
        assert_eq!(clean.get("count"), Some(&Value::Int(3)));
        assert_eq!(clean.get("note"), Some(&Value::from("see [redacted]")));
    }

    #[test]
    fn a_secret_that_is_a_number_does_not_pass_as_a_number() {
        let pin = Knows("4821");
        // The system echoes the PIN as a number, alone and inside others.
        let values = Values::new()
            .with("pin", 4821i64)
            .with("amount", Decimal::parse("4821.50").unwrap())
            .with("longer", 148_210i64)
            .with("wait", Value::Duration(Millis::new(4821)))
            .with("at", Value::Instant(Timestamp::from_unix_millis(4821)))
            .with("count", 3i64)
            .with("total", Decimal::parse("48.21").unwrap())
            .with("flag", true)
            .with("lines", vec![Value::Int(4821), Value::Int(7)]);
        let clean = pin.redact_values(&values);
        for name in ["pin", "amount", "longer", "wait", "at"] {
            assert_eq!(clean.get(name), Some(&Value::from(REDACTED)), "{name}");
        }
        assert_eq!(
            clean.get("lines"),
            Some(&Value::List(vec![Value::from(REDACTED), Value::Int(7)]))
        );
        // What does not hold the secret keeps its kind and its value.
        assert_eq!(clean.get("count"), Some(&Value::Int(3)));
        assert_eq!(
            clean.get("total"),
            Some(&Value::Decimal(Decimal::parse("48.21").unwrap()))
        );
        assert_eq!(clean.get("flag"), Some(&Value::Bool(true)));
        assert!(!pin.contains_secret(&String::from_utf8(clean.canonical_bytes()).unwrap()));

        // A date is found as it is written and as eight digits.
        let day = Value::Date(Date::new(1990, 1, 31).unwrap());
        assert_eq!(Knows("19900131").redact_value(&day), Value::from(REDACTED));
        assert_eq!(
            Knows("1990-01-31").redact_value(&day),
            Value::from(REDACTED)
        );
        assert_eq!(Knows("19900201").redact_value(&day), day);
        assert_eq!(NoSecrets.redact_value(&Value::Int(4821)), Value::Int(4821));
    }

    #[test]
    fn two_names_that_both_hold_a_secret_keep_both_values() {
        let values = Values::new()
            .with(format!("a {SECRET}"), 1i64)
            .with(SECRET, 2i64)
            .with(format!("{SECRET}{SECRET}"), 3i64)
            .with("plain", 4i64);
        let clean = Knows(SECRET).redact_values(&values);
        assert_eq!(clean.len(), values.len());
        assert_eq!(clean.get("[redacted]"), Some(&Value::Int(2)));
        assert_eq!(clean.get("[redacted][redacted]"), Some(&Value::Int(3)));
        assert_eq!(clean.get("a [redacted]"), Some(&Value::Int(1)));
        // Two names that redact alike: the second is numbered.
        let alike =
            Knows("x").redact_values(&Values::new().with("ax", 1i64).with("a[redacted]", 2i64));
        assert_eq!(alike.len(), 2);
        assert_eq!(alike.get("a[redacted]"), Some(&Value::Int(2)));
        assert_eq!(alike.get("a[redacted] (2)"), Some(&Value::Int(1)));
    }

    #[test]
    fn the_redactor_with_nothing_registered_changes_nothing() {
        assert_eq!(NoSecrets.redact(SECRET), SECRET);
        assert!(!NoSecrets.contains_secret(SECRET));
    }

    /// Test cases 1, 2, and 6 of RFC 4231.
    #[test]
    fn hmac_gives_the_standard_answers() {
        let cases: [(&[u8], &[u8], &str); 3] = [
            (
                &[0x0b; 20],
                b"Hi There",
                "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7",
            ),
            (
                b"Jefe",
                b"what do ya want for nothing?",
                "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843",
            ),
            (
                &[0xaa; 131],
                b"Test Using Larger Than Block-Size Key - Hash Key First",
                "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54",
            ),
        ];
        for (key, message, expected) in cases {
            assert_eq!(hmac_sha256(key, &[message]).to_string(), expected);
        }
        // The parts are one message, however it is cut.
        assert_eq!(
            hmac_sha256(b"Jefe", &[b"what do ya ", b"want for nothing?"]),
            hmac_sha256(b"Jefe", &[b"what do ya want for nothing?"])
        );
    }

    #[test]
    fn a_keyed_digest_differs_by_key_by_workspace_and_by_purpose() {
        struct Held([u8; 32]);
        impl KeyedDigest for Held {
            fn keyed_digest(
                &self,
                workspace: &WorkspaceId,
                purpose: DigestPurpose,
                bytes: &[u8],
            ) -> Digest {
                keyed_digest_under(&self.0, workspace, purpose, bytes)
            }
        }
        let workspace =
            |n: u8| WorkspaceId::from_parts(Timestamp::from_unix_millis(1), [n; 10]).unwrap();
        let held = Held([7; 32]);
        let base = held.keyed_digest(&workspace(1), DigestPurpose::BlobName, b"content");

        assert_eq!(
            base,
            held.keyed_digest(&workspace(1), DigestPurpose::BlobName, b"content")
        );
        assert_ne!(base, Digest::of(b"content"));
        assert_ne!(
            base,
            Held([8; 32]).keyed_digest(&workspace(1), DigestPurpose::BlobName, b"content")
        );
        assert_ne!(
            base,
            held.keyed_digest(&workspace(2), DigestPurpose::BlobName, b"content")
        );
        assert_ne!(
            base,
            held.keyed_digest(&workspace(1), DigestPurpose::RestrictedValue, b"content")
        );
        assert_ne!(
            base,
            held.keyed_digest(&workspace(1), DigestPurpose::BlobName, b"contents")
        );
        // The purpose and the bytes cannot run into each other.
        assert_ne!(
            keyed_digest_under(&[7; 32], &workspace(1), DigestPurpose::BlobName, b"x"),
            keyed_digest_under(&[7; 32], &workspace(1), DigestPurpose::RecordKey, b"x")
        );
    }
}
