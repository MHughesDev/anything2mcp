//! Findings: what discovery returns, as data.
//!
//! A finding says that something was learned about a system, how strongly, on
//! whose word, and what is weak about it. It cannot run. The rules that stop
//! a guess from passing as a fact are enforced where a finding is made, so
//! that no caller has to remember them.

use std::fmt;
use std::str::FromStr;

use crate::digest::{Digest, FieldHasher};
use crate::evidence::{Coverage, EvidenceLevel};
use crate::failure::{Failure, FailureClass, FailureCode};
use crate::name::{InvalidName, check_name};
use crate::vocabulary::vocabulary;

vocabulary! {
    /// Who vouches for a finding. The source limits how strong a finding can
    /// be: only the running system, observed, supports `discovered`.
    pub enum Source: "finding source" {
        /// The running system, observed.
        LiveSystem => "live_system",
        /// A description the system's owner publishes.
        PublishedDescription => "published_description",
        /// The system's files or code, read without running it.
        Artifact => "artifact",
        /// Someone else's account of the system.
        ThirdParty => "third_party",
        /// A person's or an agent's conclusion.
        Reasoning => "reasoning",
    }
}

impl Source {
    /// The strongest level a finding from this source can have.
    #[must_use]
    pub const fn highest_level(self) -> EvidenceLevel {
        match self {
            Self::LiveSystem => EvidenceLevel::Discovered,
            Self::PublishedDescription | Self::Artifact | Self::ThirdParty | Self::Reasoning => {
                EvidenceLevel::Inferred
            }
        }
    }
}

vocabulary! {
    /// Why a finding is weaker than it looks. A code, so that a machine can
    /// act on it; a [`Limitation`] adds a sentence, so that a person
    /// understands. The order is the documents' order.
    #[derive(PartialOrd, Ord)]
    pub enum LimitationCode: "limitation code" {
        /// The observation was cut at a size or count limit.
        Truncated => "truncated",
        /// It was cut at a time limit.
        TimeLimit => "time_limit",
        /// A sample was looked at, not the whole.
        Sampled => "sampled",
        /// The thing was seen to exist and never seen to work.
        NotExercised => "not_exercised",
        /// Something was removed before storing, such as a secret.
        Redacted => "redacted",
        /// The identity used was not allowed to see more.
        PermissionDenied => "permission_denied",
        /// An outside component did not match a recorded digest.
        UnverifiedComponent => "unverified_component",
        /// One variant was seen; others may exist.
        SingleVariant => "single_variant",
    }
}

vocabulary! {
    /// Why a finding has no subject that can be identified.
    pub enum UnknownReason: "unknown-subject reason" {
        /// Worked out from other findings.
        Derived => "derived",
        /// Never observed.
        NotObserved => "not_observed",
        /// The kind of subject is not supported.
        NotSupported => "not_supported",
        /// The identity was held back.
        Withheld => "withheld",
    }
}

vocabulary! {
    /// What the comparison of what is described with what is observed
    /// says of one endpoint (spec section 218, decision 0486).
    pub enum ComparisonResult: "comparison result" {
        /// Described and observed, exactly or through a path template.
        Matched => "matched",
        /// Described, and not seen in this observation.
        DescribedOnly => "described_only",
        /// Seen, and not in any description.
        ObservedOnly => "observed_only",
        /// The comparison could not be made, with a reason.
        Unknown => "unknown",
    }
}

vocabulary! {
    /// Why a comparison of what is described with what is observed could
    /// not be made (spec section 218, decision 0486).
    pub enum NotCompared: "reason a comparison was not made" {
        /// No observation looked at the place the description names.
        NotObserved => "not_observed",
        /// The observation of the place does not say how much it covered.
        CoverageUnknown => "coverage_unknown",
        /// The finding's pack gave no key for it.
        NoKey => "no_key",
    }
}

/// One reason a finding is weaker than it looks.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct Limitation {
    /// The reason, as a code.
    pub code: LimitationCode,
    /// The reason, as a sentence for a person.
    pub sentence: String,
}

/// What kind of thing a finding is about, such as `endpoint`, `screen`, or
/// `command`.
///
/// Kinds arrive with packs, so this is a checked name and not a closed list
/// (decision 0062). The pack that owns a kind registers the shape of what is
/// found.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FindingKind(String);

impl FindingKind {
    /// The longest a kind's name may be.
    pub const LONGEST: usize = 32;

    /// Checks `name` and makes a finding kind of it.
    ///
    /// # Errors
    ///
    /// [`InvalidName`] when the name is not lowercase letters, digits, and
    /// underscores starting with a letter, or is longer than
    /// [`FindingKind::LONGEST`].
    pub fn new(name: &str) -> Result<Self, InvalidName> {
        check_name("finding kind", name, Self::LONGEST)?;
        Ok(Self(name.to_owned()))
    }

    /// The name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for FindingKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for FindingKind {
    type Err = InvalidName;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::new(text)
    }
}

impl serde::Serialize for FindingKind {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for FindingKind {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = <String as serde::Deserialize>::deserialize(deserializer)?;
        Self::new(&text).map_err(serde::de::Error::custom)
    }
}

/// A moment, as milliseconds since the start of 1970 in UTC.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(transparent)]
pub struct Timestamp(i64);

impl Timestamp {
    /// A moment from milliseconds since the start of 1970 in UTC.
    #[must_use]
    pub const fn from_unix_millis(millis: i64) -> Self {
        Self(millis)
    }

    /// Milliseconds since the start of 1970 in UTC.
    #[must_use]
    pub const fn as_unix_millis(self) -> i64 {
        self.0
    }
}

/// A place a system answers at: an origin and a path, with no credentials,
/// no query, and no fragment in it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "RawAddress")]
pub struct Address {
    origin: String,
    path: String,
}

#[derive(serde::Deserialize)]
struct RawAddress {
    origin: String,
    path: String,
}

impl TryFrom<RawAddress> for Address {
    type Error = InvalidFinding;

    fn try_from(raw: RawAddress) -> Result<Self, Self::Error> {
        Self::new(&raw.origin, &raw.path)
    }
}

impl Address {
    /// Checks an origin and a path and makes an address of them.
    ///
    /// The origin is a scheme, `://`, and a host with an optional port. The
    /// path starts with `/`.
    ///
    /// # Errors
    ///
    /// [`InvalidFinding::AddressNotPlain`] when the origin carries user
    /// information, a path, a query, or a fragment, or when the path carries
    /// a query or a fragment. Those are where credentials travel.
    pub fn new(origin: &str, path: &str) -> Result<Self, InvalidFinding> {
        let plain_origin = origin.split_once("://").is_some_and(|(scheme, host)| {
            let scheme_ok = scheme.starts_with(|c: char| c.is_ascii_lowercase())
                && scheme
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "+.-".contains(c));
            let host_ok = !host.is_empty()
                && !host.contains(|c: char| "@/?#".contains(c) || c.is_whitespace());
            scheme_ok && host_ok
        });
        let plain_path = path.starts_with('/')
            && !path.contains(|c: char| "?#".contains(c) || c.is_whitespace());
        if plain_origin && plain_path {
            Ok(Self {
                origin: origin.to_owned(),
                path: path.to_owned(),
            })
        } else {
            Err(InvalidFinding::AddressNotPlain)
        }
    }

    /// The origin: a scheme and a host, with an optional port.
    #[must_use]
    pub fn origin(&self) -> &str {
        &self.origin
    }

    /// The path, starting with `/`.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }
}

/// What a finding is about, as one identity that can be told apart from any
/// other. "The target" is not enough: a rebuilt program at the same path, or
/// a different page at the same address, must not look like what was
/// observed.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "tag", rename_all = "snake_case")]
pub enum Subject {
    /// A file, by its content.
    File {
        /// The digest of the file's bytes.
        digest: Digest,
        /// The file's format.
        format: String,
        /// Which part, where a file holds several.
        part: Option<String>,
    },
    /// An origin and path.
    Address(Address),
    /// A running process.
    Process {
        /// The identifier the operating system gave it.
        id: u32,
        /// When it started.
        started_at: Timestamp,
        /// The digest of its program.
        program: Digest,
    },
    /// One of the runtime's own sessions.
    Session {
        /// Which session.
        session: String,
        /// What that session was attached to.
        attached_to: String,
    },
    /// A device, by what it reports about itself.
    Device {
        /// The identifier the device reports.
        reported_id: String,
        /// Its model.
        model: String,
        /// The firmware version it reports.
        firmware: String,
    },
    /// No identity, with the reason.
    Unknown {
        /// Why there is none.
        reason: UnknownReason,
    },
}

impl Subject {
    /// The name of every kind of subject, in the order the documents list
    /// them.
    pub const TAGS: &'static [&'static str] =
        &["file", "address", "process", "session", "device", "unknown"];

    /// Which kind of subject this is.
    #[must_use]
    pub const fn tag(&self) -> &'static str {
        match self {
            Self::File { .. } => "file",
            Self::Address(_) => "address",
            Self::Process { .. } => "process",
            Self::Session { .. } => "session",
            Self::Device { .. } => "device",
            Self::Unknown { .. } => "unknown",
        }
    }

    /// Adds this subject to the content a finding's identifier is made from.
    fn hash_into(&self, hasher: &mut FieldHasher) {
        hasher.field("subject", self.tag().as_bytes());
        match self {
            Self::File {
                digest,
                format,
                part,
            } => {
                hasher.field("digest", digest.as_bytes());
                hasher.field("format", format.as_bytes());
                hasher.field("part", part.as_deref().unwrap_or_default().as_bytes());
            }
            Self::Address(address) => {
                hasher.field("origin", address.origin.as_bytes());
                hasher.field("path", address.path.as_bytes());
            }
            Self::Process {
                id,
                started_at,
                program,
            } => {
                hasher.field("id", &id.to_le_bytes());
                hasher.field("started_at", &started_at.0.to_le_bytes());
                hasher.field("program", program.as_bytes());
            }
            Self::Session {
                session,
                attached_to,
            } => {
                hasher.field("session", session.as_bytes());
                hasher.field("attached_to", attached_to.as_bytes());
            }
            Self::Device {
                reported_id,
                model,
                firmware,
            } => {
                hasher.field("reported_id", reported_id.as_bytes());
                hasher.field("model", model.as_bytes());
                hasher.field("firmware", firmware.as_bytes());
            }
            Self::Unknown { reason } => hasher.field("reason", reason.as_str().as_bytes()),
        }
    }
}

/// Which discoverer produced a finding.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProducedBy {
    /// The provider's name.
    pub provider: String,
    /// The provider's version.
    pub version: String,
    /// The digest of any outside component it used.
    pub component: Option<Digest>,
}

/// Why a finding could not be made, or could not be read back.
///
/// It maps to the failure class `input`, code `finding_invalid`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidFinding {
    /// A finding is `inferred` or `discovered`. The rest of the ladder
    /// belongs to drafts and versions.
    LevelAboveDiscovered(EvidenceLevel),
    /// The source cannot support the level claimed.
    LevelAboveSource(EvidenceLevel, Source),
    /// A derived finding is never stronger than its weakest parent.
    LevelAboveParent(EvidenceLevel, EvidenceLevel),
    /// An address must carry no credentials, query, or fragment.
    AddressNotPlain,
    /// The identifier does not match the content.
    IdentifierMismatch,
}

impl fmt::Display for InvalidFinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LevelAboveDiscovered(level) => write!(
                f,
                "a finding cannot be `{level}`; a finding is `inferred` or `discovered`, and higher levels belong to drafts and versions"
            ),
            Self::LevelAboveSource(level, source) => write!(
                f,
                "a finding from `{source}` cannot be `{level}`; that source supports `{}` at most",
                source.highest_level()
            ),
            Self::LevelAboveParent(level, parent) => write!(
                f,
                "a finding derived from one that is `{parent}` cannot be `{level}`; a derived finding is never stronger than its weakest parent"
            ),
            Self::AddressNotPlain => f.write_str(
                "an address is a scheme, a host, and a path; it carries no user information, query, or fragment",
            ),
            Self::IdentifierMismatch => {
                f.write_str("the finding's identifier is not the digest of its content")
            }
        }
    }
}

impl std::error::Error for InvalidFinding {}

impl From<InvalidFinding> for Failure {
    fn from(error: InvalidFinding) -> Self {
        let next_step = match error {
            InvalidFinding::LevelAboveDiscovered(_)
            | InvalidFinding::LevelAboveSource(..)
            | InvalidFinding::LevelAboveParent(..) => {
                "Claim the weaker level, or observe the system."
            }
            InvalidFinding::AddressNotPlain => "Give the origin and the path alone.",
            InvalidFinding::IdentifierMismatch => "Discard the finding and observe again.",
        };
        Self::new(
            FailureClass::Input,
            FailureCode::FINDING_INVALID,
            error.to_string(),
            next_step,
        )
    }
}

/// Something learned about a system.
///
/// A finding is made with [`Finding::about`] and the builder it returns, and
/// in no other way. Its level is `inferred` unless a stronger one is claimed
/// and allowed; its coverage is `unknown` unless stated. Its identifier is a
/// digest of its content, so the same observation has the same identifier
/// everywhere.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "RawFinding")]
pub struct Finding {
    id: Digest,
    subject: Subject,
    kind: FindingKind,
    found: String,
    level: EvidenceLevel,
    source: Source,
    coverage: Coverage,
    limitations: Vec<Limitation>,
    derived_from: Vec<Digest>,
    produced_by: ProducedBy,
    observed_at: Timestamp,
    evidence: Vec<Digest>,
}

#[derive(serde::Deserialize)]
struct RawFinding {
    id: Digest,
    subject: Subject,
    kind: FindingKind,
    found: String,
    level: EvidenceLevel,
    source: Source,
    coverage: Coverage,
    limitations: Vec<Limitation>,
    derived_from: Vec<Digest>,
    produced_by: ProducedBy,
    observed_at: Timestamp,
    evidence: Vec<Digest>,
}

impl TryFrom<RawFinding> for Finding {
    type Error = InvalidFinding;

    /// A stored finding is checked again when it is read: its level against
    /// its source, and its identifier against its content.
    fn try_from(raw: RawFinding) -> Result<Self, Self::Error> {
        check_level(raw.level, raw.source)?;
        let mut limitations = raw.limitations;
        limitations.sort();
        limitations.dedup();
        let id = identifier(
            &raw.subject,
            &raw.kind,
            &raw.found,
            raw.source,
            raw.level,
            raw.coverage,
            &limitations,
        );
        if id != raw.id {
            return Err(InvalidFinding::IdentifierMismatch);
        }
        Ok(Self {
            id,
            subject: raw.subject,
            kind: raw.kind,
            found: raw.found,
            level: raw.level,
            source: raw.source,
            coverage: raw.coverage,
            limitations,
            derived_from: raw.derived_from,
            produced_by: raw.produced_by,
            observed_at: raw.observed_at,
            evidence: raw.evidence,
        })
    }
}

/// A level a finding may have, given its source.
fn check_level(level: EvidenceLevel, source: Source) -> Result<(), InvalidFinding> {
    if level > EvidenceLevel::Discovered {
        Err(InvalidFinding::LevelAboveDiscovered(level))
    } else if level > source.highest_level() {
        Err(InvalidFinding::LevelAboveSource(level, source))
    } else {
        Ok(())
    }
}

/// The digest of a finding's content. Who produced it, when, what it was
/// derived from, and where its evidence is stored are not content: the same
/// observation made twice is the same finding.
fn identifier(
    subject: &Subject,
    kind: &FindingKind,
    found: &str,
    source: Source,
    level: EvidenceLevel,
    coverage: Coverage,
    limitations: &[Limitation],
) -> Digest {
    let mut hasher = FieldHasher::new();
    hasher.field("anything2mcp", b"finding 1");
    subject.hash_into(&mut hasher);
    hasher.field("kind", kind.as_str().as_bytes());
    hasher.field("found", found.as_bytes());
    hasher.field("source", source.as_str().as_bytes());
    hasher.field("level", level.as_str().as_bytes());
    hasher.field("coverage", coverage.as_str().as_bytes());
    for limitation in limitations {
        hasher.field("limitation", limitation.code.as_str().as_bytes());
        hasher.field("sentence", limitation.sentence.as_bytes());
    }
    hasher.finish()
}

impl Finding {
    /// Begins a finding about `subject`.
    ///
    /// `found` is the thing itself, in the form the pack that owns `kind`
    /// defines. The kernel stores it and never reads it.
    #[must_use]
    pub fn about(
        subject: Subject,
        kind: FindingKind,
        found: impl Into<String>,
        source: Source,
        produced_by: ProducedBy,
        observed_at: Timestamp,
    ) -> FindingBuilder {
        FindingBuilder {
            subject,
            kind,
            found: found.into(),
            source,
            produced_by,
            observed_at,
            level: EvidenceLevel::default(),
            coverage: Coverage::default(),
            limitations: Vec::new(),
            parents: Vec::new(),
            evidence: Vec::new(),
        }
    }

    /// The digest of the finding's content.
    #[must_use]
    pub fn id(&self) -> Digest {
        self.id
    }

    /// What the finding is about.
    #[must_use]
    pub fn subject(&self) -> &Subject {
        &self.subject
    }

    /// What kind of thing was found.
    #[must_use]
    pub fn kind(&self) -> &FindingKind {
        &self.kind
    }

    /// The thing itself, in its pack's form.
    #[must_use]
    pub fn found(&self) -> &str {
        &self.found
    }

    /// `inferred` or `discovered`.
    #[must_use]
    pub fn level(&self) -> EvidenceLevel {
        self.level
    }

    /// Who vouches for it.
    #[must_use]
    pub fn source(&self) -> Source {
        self.source
    }

    /// How much of what could have been looked at was looked at.
    #[must_use]
    pub fn coverage(&self) -> Coverage {
        self.coverage
    }

    /// Why it is weaker than it looks, in a fixed order.
    #[must_use]
    pub fn limitations(&self) -> &[Limitation] {
        &self.limitations
    }

    /// The findings it was worked out from.
    #[must_use]
    pub fn derived_from(&self) -> &[Digest] {
        &self.derived_from
    }

    /// Which discoverer produced it.
    #[must_use]
    pub fn produced_by(&self) -> &ProducedBy {
        &self.produced_by
    }

    /// When it was observed.
    #[must_use]
    pub fn observed_at(&self) -> Timestamp {
        self.observed_at
    }

    /// The stored observations behind it.
    #[must_use]
    pub fn evidence(&self) -> &[Digest] {
        &self.evidence
    }
}

/// Collects what is known about a finding and checks it when the finding is
/// made.
#[derive(Debug, Clone)]
pub struct FindingBuilder {
    subject: Subject,
    kind: FindingKind,
    found: String,
    source: Source,
    produced_by: ProducedBy,
    observed_at: Timestamp,
    level: EvidenceLevel,
    coverage: Coverage,
    limitations: Vec<Limitation>,
    parents: Vec<(Digest, EvidenceLevel)>,
    evidence: Vec<Digest>,
}

impl FindingBuilder {
    /// Claims a level. Without this the finding is `inferred`.
    #[must_use]
    pub fn level(mut self, level: EvidenceLevel) -> Self {
        self.level = level;
        self
    }

    /// States the coverage. Without this it is `unknown`.
    #[must_use]
    pub fn coverage(mut self, coverage: Coverage) -> Self {
        self.coverage = coverage;
        self
    }

    /// Adds a reason the finding is weaker than it looks.
    #[must_use]
    pub fn limitation(mut self, code: LimitationCode, sentence: impl Into<String>) -> Self {
        self.limitations.push(Limitation {
            code,
            sentence: sentence.into(),
        });
        self
    }

    /// Says the finding was worked out from another.
    #[must_use]
    pub fn derived_from(mut self, parent: &Finding) -> Self {
        self.parents.push((parent.id, parent.level));
        self
    }

    /// Adds a reference to a stored observation.
    #[must_use]
    pub fn evidence(mut self, blob: Digest) -> Self {
        self.evidence.push(blob);
        self
    }

    /// Makes the finding.
    ///
    /// # Errors
    ///
    /// [`InvalidFinding`] when the level claimed is above `discovered`, above
    /// what the source supports, or above the weakest finding this one was
    /// derived from.
    pub fn build(self) -> Result<Finding, InvalidFinding> {
        check_level(self.level, self.source)?;
        if let Some(weakest) = self.parents.iter().map(|(_, level)| *level).min()
            && self.level > weakest
        {
            return Err(InvalidFinding::LevelAboveParent(self.level, weakest));
        }
        let mut limitations = self.limitations;
        limitations.sort();
        limitations.dedup();
        let id = identifier(
            &self.subject,
            &self.kind,
            &self.found,
            self.source,
            self.level,
            self.coverage,
            &limitations,
        );
        Ok(Finding {
            id,
            subject: self.subject,
            kind: self.kind,
            found: self.found,
            level: self.level,
            source: self.source,
            coverage: self.coverage,
            limitations,
            derived_from: self.parents.into_iter().map(|(id, _)| id).collect(),
            produced_by: self.produced_by,
            observed_at: self.observed_at,
            evidence: self.evidence,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Address, Finding, FindingBuilder, FindingKind, InvalidFinding, LimitationCode, ProducedBy,
        Source, Subject, Timestamp, UnknownReason,
    };
    use crate::{Coverage, Digest, EvidenceLevel, Failure, FailureClass, FailureCode};

    fn by() -> ProducedBy {
        ProducedBy {
            provider: "example_probe".to_owned(),
            version: "0.0.1".to_owned(),
            component: None,
        }
    }

    fn endpoint(source: Source) -> FindingBuilder {
        let subject =
            Subject::Address(Address::new("https://app.example.com", "/api/orders").unwrap());
        let kind = FindingKind::new("endpoint").unwrap();
        Finding::about(
            subject,
            kind,
            r#"{"method":"GET"}"#,
            source,
            by(),
            Timestamp::from_unix_millis(1_000),
        )
    }

    #[test]
    fn what_is_not_stated_takes_the_weakest_value() {
        let finding = endpoint(Source::LiveSystem).build().unwrap();
        assert_eq!(finding.level(), EvidenceLevel::Inferred);
        assert_eq!(finding.coverage(), Coverage::Unknown);
        assert!(finding.limitations().is_empty());
    }

    #[test]
    fn only_the_live_system_supports_discovered() {
        let seen = endpoint(Source::LiveSystem)
            .level(EvidenceLevel::Discovered)
            .build()
            .unwrap();
        assert_eq!(seen.level(), EvidenceLevel::Discovered);
        for source in [
            Source::PublishedDescription,
            Source::Artifact,
            Source::ThirdParty,
            Source::Reasoning,
        ] {
            let refused = endpoint(source)
                .level(EvidenceLevel::Discovered)
                .build()
                .unwrap_err();
            assert_eq!(
                refused,
                InvalidFinding::LevelAboveSource(EvidenceLevel::Discovered, source)
            );
            assert!(endpoint(source).build().is_ok());
        }
    }

    #[test]
    fn a_finding_is_never_above_discovered() {
        for level in EvidenceLevel::ALL
            .iter()
            .copied()
            .filter(|level| *level > EvidenceLevel::Discovered)
        {
            let refused = endpoint(Source::LiveSystem)
                .level(level)
                .build()
                .unwrap_err();
            assert_eq!(refused, InvalidFinding::LevelAboveDiscovered(level));
        }
    }

    #[test]
    fn a_derived_finding_is_never_stronger_than_its_weakest_parent() {
        let seen = endpoint(Source::LiveSystem)
            .level(EvidenceLevel::Discovered)
            .build()
            .unwrap();
        let guessed = endpoint(Source::Reasoning).build().unwrap();
        let refused = endpoint(Source::LiveSystem)
            .level(EvidenceLevel::Discovered)
            .derived_from(&seen)
            .derived_from(&guessed)
            .build()
            .unwrap_err();
        assert_eq!(
            refused,
            InvalidFinding::LevelAboveParent(EvidenceLevel::Discovered, EvidenceLevel::Inferred)
        );
        let allowed = endpoint(Source::LiveSystem)
            .derived_from(&seen)
            .derived_from(&guessed)
            .build()
            .unwrap();
        assert_eq!(allowed.derived_from(), [seen.id(), guessed.id()]);
    }

    #[test]
    fn the_same_observation_has_the_same_identifier() {
        let first = endpoint(Source::LiveSystem)
            .limitation(LimitationCode::Sampled, "One page of results was read.")
            .limitation(
                LimitationCode::TimeLimit,
                "The capture ran for five minutes.",
            )
            .evidence(Digest::of(b"one capture"))
            .build()
            .unwrap();
        // Made again later, by another provider, from other evidence, with the
        // limitations given in another order.
        let subject =
            Subject::Address(Address::new("https://app.example.com", "/api/orders").unwrap());
        let other = ProducedBy {
            provider: "another_probe".to_owned(),
            version: "2.0.0".to_owned(),
            component: Some(Digest::of(b"a component")),
        };
        let second = Finding::about(
            subject,
            FindingKind::new("endpoint").unwrap(),
            r#"{"method":"GET"}"#,
            Source::LiveSystem,
            other,
            Timestamp::from_unix_millis(9_000),
        )
        .limitation(
            LimitationCode::TimeLimit,
            "The capture ran for five minutes.",
        )
        .limitation(LimitationCode::Sampled, "One page of results was read.")
        .evidence(Digest::of(b"another capture"))
        .build()
        .unwrap();
        assert_eq!(first.id(), second.id());
    }

    #[test]
    fn a_change_to_any_part_of_the_content_changes_the_identifier() {
        let base = endpoint(Source::LiveSystem).build().unwrap().id();
        let other_path =
            Subject::Address(Address::new("https://app.example.com", "/api/items").unwrap());
        let kind = || FindingKind::new("endpoint").unwrap();
        let at = Timestamp::from_unix_millis(1_000);
        let changed = [
            Finding::about(
                other_path,
                kind(),
                r#"{"method":"GET"}"#,
                Source::LiveSystem,
                by(),
                at,
            )
            .build(),
            Finding::about(
                Subject::Unknown {
                    reason: UnknownReason::NotObserved,
                },
                kind(),
                r#"{"method":"GET"}"#,
                Source::LiveSystem,
                by(),
                at,
            )
            .build(),
            endpoint(Source::Artifact).build(),
            endpoint(Source::LiveSystem)
                .level(EvidenceLevel::Discovered)
                .build(),
            endpoint(Source::LiveSystem)
                .coverage(Coverage::Partial)
                .build(),
            endpoint(Source::LiveSystem)
                .limitation(LimitationCode::Sampled, "A sample.")
                .build(),
        ];
        let mut seen = vec![base];
        for finding in changed {
            let id = finding.unwrap().id();
            assert!(
                !seen.contains(&id),
                "two different findings share an identifier"
            );
            seen.push(id);
        }
        let subject =
            Subject::Address(Address::new("https://app.example.com", "/api/orders").unwrap());
        let other_kind = Finding::about(
            subject.clone(),
            FindingKind::new("screen").unwrap(),
            r#"{"method":"GET"}"#,
            Source::LiveSystem,
            by(),
            at,
        );
        let other_found = Finding::about(
            subject,
            kind(),
            r#"{"method":"PUT"}"#,
            Source::LiveSystem,
            by(),
            at,
        );
        for finding in [other_kind, other_found] {
            assert!(!seen.contains(&finding.build().unwrap().id()));
        }
    }

    #[test]
    fn an_address_carries_no_credentials() {
        for (origin, path) in [
            ("https://user:secret@app.example.com", "/api"),
            ("https://app.example.com/extra", "/api"),
            ("https://app.example.com", "/api?token=secret"),
            ("https://app.example.com", "/api#part"),
            ("https://app.example.com", "api"),
            ("app.example.com", "/api"),
            ("https://", "/api"),
            ("HTTPS://app.example.com", "/api"),
        ] {
            assert_eq!(
                Address::new(origin, path),
                Err(InvalidFinding::AddressNotPlain),
                "{origin} {path}"
            );
        }
        let address = Address::new("https://app.example.com:8443", "/api/orders").unwrap();
        assert_eq!(
            (address.origin(), address.path()),
            ("https://app.example.com:8443", "/api/orders")
        );
        let stored = r#"{"origin":"https://user:secret@app.example.com","path":"/api"}"#;
        assert!(serde_json::from_str::<Address>(stored).is_err());
    }

    #[test]
    fn a_stored_finding_is_checked_again_when_it_is_read() {
        let finding = endpoint(Source::LiveSystem)
            .level(EvidenceLevel::Discovered)
            .coverage(Coverage::Partial)
            .limitation(LimitationCode::Sampled, "A sample.")
            .evidence(Digest::of(b"capture"))
            .build()
            .unwrap();
        let json = serde_json::to_value(&finding).unwrap();
        assert_eq!(json["subject"]["tag"], "address");
        assert_eq!(json["level"], "discovered");
        assert_eq!(
            serde_json::from_value::<Finding>(json.clone()).unwrap(),
            finding
        );

        let mut stronger = json.clone();
        stronger["level"] = "approved".into();
        assert!(serde_json::from_value::<Finding>(stronger).is_err());

        let mut wrong_source = json.clone();
        wrong_source["source"] = "reasoning".into();
        assert!(serde_json::from_value::<Finding>(wrong_source).is_err());

        let mut edited = json;
        edited["found"] = r#"{"method":"DELETE"}"#.into();
        let error = serde_json::from_value::<Finding>(edited).unwrap_err();
        assert!(
            error.to_string().contains("not the digest of its content"),
            "{error}"
        );
    }

    #[test]
    fn every_kind_of_subject_is_read_back_as_written() {
        let subjects = [
            Subject::File {
                digest: Digest::of(b"file"),
                format: "csv".to_owned(),
                part: Some("sheet_1".to_owned()),
            },
            Subject::Address(Address::new("https://app.example.com", "/").unwrap()),
            Subject::Process {
                id: 42,
                started_at: Timestamp::from_unix_millis(5),
                program: Digest::of(b"program"),
            },
            Subject::Session {
                session: "s_1".to_owned(),
                attached_to: "example".to_owned(),
            },
            Subject::Device {
                reported_id: "d_1".to_owned(),
                model: "example-controller".to_owned(),
                firmware: "1.2.0".to_owned(),
            },
            Subject::Unknown {
                reason: UnknownReason::Withheld,
            },
        ];
        let tags: Vec<&str> = subjects.iter().map(Subject::tag).collect();
        assert_eq!(tags, Subject::TAGS);
        for subject in subjects {
            let json = serde_json::to_value(&subject).unwrap();
            assert_eq!(json["tag"], subject.tag());
            assert_eq!(serde_json::from_value::<Subject>(json).unwrap(), subject);
        }
    }

    #[test]
    fn a_refused_finding_becomes_an_input_failure() {
        let refused = endpoint(Source::Reasoning)
            .level(EvidenceLevel::Discovered)
            .build()
            .unwrap_err();
        let failure: Failure = refused.into();
        assert_eq!(failure.class(), FailureClass::Input);
        assert_eq!(failure.code(), &FailureCode::FINDING_INVALID);
        assert!(failure.message().contains("`reasoning`"), "{failure}");
    }

    #[test]
    fn a_badly_formed_finding_kind_is_refused() {
        for bad in ["", "End Point", "endpoint-list", &"k".repeat(33)] {
            assert!(FindingKind::new(bad).is_err(), "{bad}");
        }
        assert_eq!(FindingKind::new("endpoint").unwrap().as_str(), "endpoint");
    }
}
