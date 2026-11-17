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

    /// The failure with its sentence and its next step redacted. The class
    /// and the code are names and are kept.
    fn redact_failure(&self, failure: &Failure) -> Failure {
        Failure::new(
            failure.class(),
            failure.code().clone(),
            self.redact(failure.message()),
            self.redact(failure.next_step()),
        )
    }

    /// The value with every text in it redacted, at any depth. A field's
    /// name is redacted too, because a system chooses the names it returns.
    fn redact_value(&self, value: &Value) -> Value {
        match value {
            Value::Text(text) => Value::Text(self.redact(text)),
            Value::List(items) => {
                Value::List(items.iter().map(|item| self.redact_value(item)).collect())
            }
            Value::Record(values) => Value::Record(self.redact_values(values)),
            other => other.clone(),
        }
    }

    /// The named values with every text in them redacted.
    fn redact_values(&self, values: &Values) -> Values {
        values
            .iter()
            .map(|(name, value)| (self.redact(name), self.redact_value(value)))
            .collect()
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
    let workspace_key = hmac_sha256(key, &[b"anything2mcp workspace digest key 1", workspace.as_bytes()]);
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
        Digest, Failure, FailureClass, FailureCode, Timestamp, Value, Values, WorkspaceId,
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
        assert_eq!(redactor.redact(&format!("a {SECRET} b {SECRET}")), "a [redacted] b [redacted]");
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
        assert_eq!(clean.message(), "The system answered: bad token [redacted].");
        assert_eq!(clean.next_step(), "Sign in again; do not send [redacted].");
        assert!(!format!("{clean:?}").contains(SECRET));
    }

    #[test]
    fn values_are_redacted_at_every_depth_and_in_their_names() {
        let values = Values::new()
            .with("note", format!("see {SECRET}"))
            .with("count", 3i64)
            .with(
                "lines",
                vec![Value::from(SECRET), Value::Record(Values::new().with(SECRET, SECRET))],
            );
        let clean = Knows(SECRET).redact_values(&values);
        assert!(!format!("{clean:?}").contains(SECRET));
        assert!(!Knows(SECRET).contains_secret(&String::from_utf8(clean.canonical_bytes()).unwrap()));
        assert_eq!(clean.get("count"), Some(&Value::Int(3)));
        assert_eq!(clean.get("note"), Some(&Value::from("see [redacted]")));
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
            fn keyed_digest(&self, workspace: &WorkspaceId, purpose: DigestPurpose, bytes: &[u8]) -> Digest {
                keyed_digest_under(&self.0, workspace, purpose, bytes)
            }
        }
        let workspace = |n: u8| WorkspaceId::from_parts(Timestamp::from_unix_millis(1), [n; 10]).unwrap();
        let held = Held([7; 32]);
        let base = held.keyed_digest(&workspace(1), DigestPurpose::BlobName, b"content");

        assert_eq!(base, held.keyed_digest(&workspace(1), DigestPurpose::BlobName, b"content"));
        assert_ne!(base, Digest::of(b"content"));
        assert_ne!(base, Held([8; 32]).keyed_digest(&workspace(1), DigestPurpose::BlobName, b"content"));
        assert_ne!(base, held.keyed_digest(&workspace(2), DigestPurpose::BlobName, b"content"));
        assert_ne!(base, held.keyed_digest(&workspace(1), DigestPurpose::RestrictedValue, b"content"));
        assert_ne!(base, held.keyed_digest(&workspace(1), DigestPurpose::BlobName, b"contents"));
        // The purpose and the bytes cannot run into each other.
        assert_ne!(
            keyed_digest_under(&[7; 32], &workspace(1), DigestPurpose::BlobName, b"x"),
            keyed_digest_under(&[7; 32], &workspace(1), DigestPurpose::RecordKey, b"x")
        );
    }
}
