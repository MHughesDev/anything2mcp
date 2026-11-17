//! Failures: a class, a stable code, a plain sentence, and a next step.

use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

use crate::digest::InvalidDigest;
use crate::name::{InvalidName, UnknownName, check_name, is_name};
use crate::vocabulary::vocabulary;

vocabulary! {
    /// What kind of failure stopped a call (spec section 207). The class alone
    /// decides what happens next: whether to retry, stop, reconcile, or ask a
    /// person. The list is closed; a new kind of system maps its failures to
    /// these.
    pub enum FailureClass: "failure class" {
        /// The policy engine refused: no grant, over a limit, a stale approval.
        Policy => "policy",
        /// Inputs or a source file failed validation; nothing ran.
        Input => "input",
        /// The session ended and could not renew itself.
        SessionExpired => "session_expired",
        /// A bot defence or an edge refused the request.
        EdgeBlocked => "edge_blocked",
        /// The system no longer matches what the target files describe.
        Drift => "drift",
        /// Something appeared that no rule covers.
        UnknownVariant => "unknown_variant",
        /// The system refused the input with its own validation message.
        Rejected => "rejected",
        /// The value read back differs from the value intended.
        Mismatch => "mismatch",
        /// A write may or may not have happened.
        PossibleCommit => "possible_commit",
        /// A record no longer matches what the preview saw.
        Stale => "stale",
        /// A timeout or a lost connection that may pass.
        Transient => "transient",
        /// A queue is full, a record is locked, or no runner is free.
        Busy => "busy",
    }
}

/// The stable code of one failure within its class, such as `needs_login`.
///
/// Codes are not a closed list: each crate names the failures it can produce.
/// A code never changes once a release has carried it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FailureCode(Cow<'static, str>);

impl FailureCode {
    /// The longest a code may be.
    pub const LONGEST: usize = 64;

    /// A file, an input, or a name does not have the shape its schema requires.
    pub const SCHEMA_INVALID: Self = Self::fixed("schema_invalid");

    /// A finding claims more than its source, its parents, or its content allow.
    pub const FINDING_INVALID: Self = Self::fixed("finding_invalid");

    /// A decimal, a date, a key part, or a moment cannot be represented.
    pub const VALUE_OUT_OF_RANGE: Self = Self::fixed("value_out_of_range");

    /// A plan's bytes do not match the hash it was published with.
    pub const PLAN_HASH_MISMATCH: Self = Self::fixed("plan_hash_mismatch");

    /// A plan's format is not one this engine loads.
    pub const PLAN_FORMAT_UNSUPPORTED: Self = Self::fixed("plan_format_unsupported");

    /// A plan's bytes match its hash and are not a well-formed plan.
    pub const PLAN_UNREADABLE: Self = Self::fixed("plan_unreadable");

    /// A payload in a plan does not decode as the type its reader expects.
    pub const PAYLOAD_UNREADABLE: Self = Self::fixed("payload_unreadable");

    /// Data holds something the plan's encoding cannot carry, such as a float.
    pub const PAYLOAD_NOT_ENCODABLE: Self = Self::fixed("payload_not_encodable");

    /// Makes a code from a name written in the source.
    ///
    /// # Panics
    ///
    /// When the name is not well formed. Used in a `const`, as it is meant to
    /// be, that is an error at compile time and never at run time.
    #[must_use]
    pub const fn fixed(name: &'static str) -> Self {
        assert!(
            is_name(name, Self::LONGEST),
            "a failure code is lowercase letters, digits, and underscores, starting with a letter"
        );
        Self(Cow::Borrowed(name))
    }

    /// Checks `name` and makes a code of it.
    ///
    /// # Errors
    ///
    /// [`InvalidName`] when the name is not lowercase letters, digits, and
    /// underscores starting with a letter, or is longer than
    /// [`FailureCode::LONGEST`].
    pub fn new(name: &str) -> Result<Self, InvalidName> {
        check_name("failure code", name, Self::LONGEST)?;
        Ok(Self(Cow::Owned(name.to_owned())))
    }

    /// The code as it appears in results, the ledger, and messages.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for FailureCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for FailureCode {
    type Err = InvalidName;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::new(text)
    }
}

impl serde::Serialize for FailureCode {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for FailureCode {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = <String as serde::Deserialize>::deserialize(deserializer)?;
        Self::new(&text).map_err(serde::de::Error::custom)
    }
}

/// One failure, as a caller sees it: a class, a stable code, a plain
/// sentence, and what to do next (technical reference, "Failure codes").
///
/// The sentence and the next step are shown to callers and written to logs.
/// They must never contain a secret; build them from names and counts, not
/// from values a system returned.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Failure {
    class: FailureClass,
    code: FailureCode,
    message: String,
    next_step: String,
}

impl Failure {
    /// Makes a failure.
    #[must_use]
    pub fn new(
        class: FailureClass,
        code: FailureCode,
        message: impl Into<String>,
        next_step: impl Into<String>,
    ) -> Self {
        Self {
            class,
            code,
            message: message.into(),
            next_step: next_step.into(),
        }
    }

    /// The class, which alone decides what happens next.
    #[must_use]
    pub fn class(&self) -> FailureClass {
        self.class
    }

    /// The stable code within the class.
    #[must_use]
    pub fn code(&self) -> &FailureCode {
        &self.code
    }

    /// What went wrong, in one plain sentence.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// What the caller should do next.
    #[must_use]
    pub fn next_step(&self) -> &str {
        &self.next_step
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}: {}", self.class, self.code, self.message)
    }
}

impl std::error::Error for Failure {}

impl From<UnknownName> for Failure {
    fn from(error: UnknownName) -> Self {
        let next_step = format!("Use one of: {}.", error.known().join(", "));
        Self::new(
            FailureClass::Input,
            FailureCode::SCHEMA_INVALID,
            error.to_string(),
            next_step,
        )
    }
}

impl From<InvalidDigest> for Failure {
    fn from(error: InvalidDigest) -> Self {
        Self::new(
            FailureClass::Input,
            FailureCode::SCHEMA_INVALID,
            error.to_string(),
            "Write the digest as 64 lowercase hexadecimal characters.",
        )
    }
}

impl From<InvalidName> for Failure {
    fn from(error: InvalidName) -> Self {
        let next_step = error.next_step();
        Self::new(
            FailureClass::Input,
            FailureCode::SCHEMA_INVALID,
            error.to_string(),
            next_step,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{Failure, FailureClass, FailureCode};
    use crate::{Mode, RunState};

    #[test]
    fn a_failure_carries_its_four_parts() {
        let failure = Failure::new(
            FailureClass::SessionExpired,
            FailureCode::new("needs_login").unwrap(),
            "The session for this target has ended.",
            "Sign in again, then resume the run.",
        );
        assert_eq!(failure.class(), FailureClass::SessionExpired);
        assert_eq!(failure.code().as_str(), "needs_login");
        assert_eq!(
            failure.to_string(),
            "session_expired/needs_login: The session for this target has ended."
        );
        assert_eq!(failure.next_step(), "Sign in again, then resume the run.");
    }

    #[test]
    fn a_failure_is_read_and_written_by_its_names() {
        let failure = Failure::new(
            FailureClass::Busy,
            FailureCode::new("record_locked").unwrap(),
            "Another run holds this record.",
            "Try again in a minute.",
        );
        let json = serde_json::to_value(&failure).unwrap();
        assert_eq!(json["class"], "busy");
        assert_eq!(json["code"], "record_locked");
        assert_eq!(serde_json::from_value::<Failure>(json).unwrap(), failure);
    }

    #[test]
    fn a_badly_formed_code_is_refused() {
        for bad in ["", "Needs_Login", "needs-login", "9lives", &"c".repeat(65)] {
            assert!(FailureCode::new(bad).is_err(), "{bad}");
        }
        assert!(serde_json::from_str::<FailureCode>("\"NOT A CODE\"").is_err());
    }

    #[test]
    fn a_fixed_code_equals_the_same_code_made_at_run_time() {
        assert_eq!(
            FailureCode::SCHEMA_INVALID,
            FailureCode::new("schema_invalid").unwrap()
        );
    }

    #[test]
    fn an_unknown_word_becomes_an_input_failure_that_lists_the_known_ones() {
        let failure: Failure = "paused".parse::<RunState>().unwrap_err().into();
        assert_eq!(failure.class(), FailureClass::Input);
        assert_eq!(failure.code(), &FailureCode::SCHEMA_INVALID);
        assert!(
            failure.message().contains("`paused` is not a run state"),
            "{failure}"
        );
        assert!(
            failure.next_step().contains("needs_approval"),
            "{}",
            failure.next_step()
        );
    }

    #[test]
    fn a_badly_formed_name_becomes_an_input_failure() {
        let failure: Failure = Mode::new("Page HTTP").unwrap_err().into();
        assert_eq!(failure.class(), FailureClass::Input);
        assert_eq!(failure.code(), &FailureCode::SCHEMA_INVALID);
    }
}
