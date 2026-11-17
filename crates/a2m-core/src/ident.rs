//! Names and identifiers: the checked names that arrive with packs and
//! target files, the opaque identifiers the store gives out, and who a
//! system sees.
//!
//! Nothing here reads a clock or a random source. An identifier is made from
//! a moment and random bytes that the caller supplies.

use std::fmt;
use std::str::FromStr;

use crate::failure::{Failure, FailureClass, FailureCode};
use crate::finding::Timestamp;
use crate::name::{check_dotted_name, checked_name};

checked_name! {
    /// The name of a target: one system a developer builds tools over. It is
    /// the prefix of every tool name of that target.
    ///
    /// Lowercase letters, digits, and underscores, starting with a letter.
    pub struct TargetName: "target", 32;
}

checked_name! {
    /// The name of an operation or a pipeline, which is also its tool name:
    /// `{target}_{module}_{object}_{verb}`.
    ///
    /// Only the shape of a name is checked here. The parts cannot be told
    /// apart by reading the name, because each may hold an underscore; the
    /// compiler, which knows the target and its modules, checks them.
    pub struct OperationName: "operation", 64;
}

impl OperationName {
    /// Whether the name starts with `target` and an underscore, as every
    /// tool name of that target must.
    #[must_use]
    pub fn belongs_to(&self, target: &TargetName) -> bool {
        self.as_str()
            .strip_prefix(target.as_str())
            .is_some_and(|rest| rest.len() > 1 && rest.starts_with('_'))
    }
}

checked_name! {
    /// The name of one surface of a target, unique within the target.
    pub struct SurfaceName: "surface", 32;
}

checked_name! {
    /// The name of an environment: which copy of the system, such as `test`
    /// or `production`.
    pub struct Environment: "environment", 32;
}

checked_name! {
    /// A kind of surface, such as `web` or `http`. A pack registers each kind
    /// it serves; the kernel holds no list (decision 0062).
    pub struct SurfaceKind: "surface kind", 32;
}

checked_name! {
    /// A kind of step, such as `locate`, `set`, or `request`. A step kind is
    /// a name within its mode, registered with the lowering that produces it
    /// and known to the driver of that mode (decision 0062).
    pub struct StepKind: "step kind", 32;
}

checked_name! {
    /// What a binding does, such as `read`, `upsert`, or `call`. A pack
    /// registers a lowering for each verb of each mode it serves.
    ///
    /// The engine never reads a verb's spelling. How a binding is treated is
    /// stated by its lowered steps and by its operation (decision 0068).
    pub struct Verb: "verb", 32;
}

checked_name! {
    /// A kind of target file, such as `operation`, `screen`, or `endpoint`.
    /// The kernel owns some; a pack registers the rest.
    pub struct FileKindName: "file kind", 32;
}

checked_name! {
    /// A kind of control in an interface, such as `combobox` or `grid`. A
    /// pack registers the controls it can operate.
    pub struct ControlName: "control", 32;
}

checked_name! {
    /// The name of a dedicated account that a system is reached as, as the
    /// settings name it. It is a name, never a credential.
    pub struct AccountName: "account", 64;
}

checked_name! {
    /// The stable code of a lint rule: its owner and the rule, each a name,
    /// joined by one dot, such as `kernel.no_fixed_wait`. The owner is
    /// `kernel` or the provider that registers the rule.
    pub struct LintCode: "lint code", 64, check_dotted_name;
}

impl LintCode {
    /// Who registered the rule: `kernel`, or a provider.
    #[must_use]
    pub fn owner(&self) -> &str {
        self.as_str().split_once('.').map_or("", |(owner, _)| owner)
    }

    /// The rule's name within its owner.
    #[must_use]
    pub fn rule(&self) -> &str {
        self.as_str().split_once('.').map_or("", |(_, rule)| rule)
    }
}

/// Text that is not an identifier, or a moment an identifier cannot hold.
///
/// It maps to the failure class `input`: code `schema_invalid` for text, and
/// `value_out_of_range` for a moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidId {
    /// The text is not the identifier's prefix, an underscore, and 32
    /// lowercase hexadecimal characters.
    Malformed {
        /// What kind of identifier was expected, such as "run".
        kind: &'static str,
        /// The prefix that kind of identifier starts with.
        prefix: &'static str,
    },
    /// The moment is before 1970 or too far ahead to fit in an identifier.
    MomentOutOfRange,
    /// The text is not three numbers joined by dots.
    VersionMalformed,
    /// The text names neither a person nor an account.
    IdentityMalformed,
}

impl fmt::Display for InvalidId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed { kind, prefix } => write!(
                f,
                "a {kind} identifier is `{prefix}_` and 32 lowercase hexadecimal characters; this text is not"
            ),
            Self::MomentOutOfRange => {
                f.write_str("an identifier holds a moment from 1970 to the year 10889; this one is outside it")
            }
            Self::VersionMalformed => {
                f.write_str("a version is three whole numbers joined by dots, such as `1.4.0`; this text is not")
            }
            Self::IdentityMalformed => f.write_str(
                "an identity is `person:` and a principal identifier, or `account:` and an account name; this text is neither",
            ),
        }
    }
}

impl std::error::Error for InvalidId {}

impl From<InvalidId> for Failure {
    fn from(error: InvalidId) -> Self {
        let (code, next_step) = match error {
            InvalidId::Malformed { .. } => (
                FailureCode::SCHEMA_INVALID,
                "Use the identifier exactly as it was given.",
            ),
            InvalidId::MomentOutOfRange => (
                FailureCode::VALUE_OUT_OF_RANGE,
                "Check the clock of the machine that made the identifier.",
            ),
            InvalidId::VersionMalformed => (
                FailureCode::SCHEMA_INVALID,
                "Write the version as three numbers, such as `1.4.0`.",
            ),
            InvalidId::IdentityMalformed => (
                FailureCode::SCHEMA_INVALID,
                "Write `person:` or `account:` and the name.",
            ),
        };
        Self::new(FailureClass::Input, code, error.to_string(), next_step)
    }
}

/// The value of one lowercase hexadecimal character.
const fn nibble(character: u8) -> Option<u8> {
    match character {
        b'0'..=b'9' => Some(character - b'0'),
        b'a'..=b'f' => Some(character - b'a' + 10),
        _ => None,
    }
}

/// The 16 bytes of a time-ordered identifier: 48 bits of milliseconds since
/// 1970, then random bits, with the version and variant marks of a version 7
/// UUID so that a store's own identifier type accepts it.
fn time_ordered(at: Timestamp, random: [u8; 10]) -> Result<[u8; 16], InvalidId> {
    let millis = u64::try_from(at.as_unix_millis()).map_err(|_| InvalidId::MomentOutOfRange)?;
    if millis >= 1 << 48 {
        return Err(InvalidId::MomentOutOfRange);
    }
    let mut bytes = [0u8; 16];
    bytes[..6].copy_from_slice(&millis.to_be_bytes()[2..]);
    bytes[6..].copy_from_slice(&random);
    bytes[6] = (bytes[6] & 0x0f) | 0x70;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(bytes)
}

/// Defines an opaque identifier: 16 bytes, written as a prefix, an
/// underscore, and 32 lowercase hexadecimal characters.
macro_rules! opaque_id {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident: $kind:literal, $prefix:literal;
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        $vis struct $name([u8; 16]);

        impl $name {
            /// What the identifier starts with when written, before the
            /// underscore.
            pub const PREFIX: &'static str = $prefix;

            /// Makes an identifier from the moment it is made at and ten
            /// random bytes. Both come from the caller: this crate reads no
            /// clock and no random source. Identifiers made later sort later.
            ///
            /// # Errors
            ///
            /// [`InvalidId::MomentOutOfRange`] when the moment is before 1970
            /// or does not fit in 48 bits of milliseconds.
            pub fn from_parts(at: Timestamp, random: [u8; 10]) -> Result<Self, InvalidId> {
                time_ordered(at, random).map(Self)
            }

            /// An identifier from its 16 bytes, as a store holds it.
            #[must_use]
            pub const fn from_bytes(bytes: [u8; 16]) -> Self {
                Self(bytes)
            }

            /// The identifier's 16 bytes.
            #[must_use]
            pub const fn as_bytes(&self) -> &[u8; 16] {
                &self.0
            }

            /// The moment held in the first six bytes: when the identifier
            /// was made, if it was made by `from_parts`.
            #[must_use]
            pub fn made_at(&self) -> Timestamp {
                let mut millis = [0u8; 8];
                millis[2..].copy_from_slice(&self.0[..6]);
                Timestamp::from_unix_millis(i64::from_be_bytes(millis))
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(Self::PREFIX)?;
                f.write_str("_")?;
                for byte in self.0 {
                    write!(f, "{byte:02x}")?;
                }
                Ok(())
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({self})", stringify!($name))
            }
        }

        impl FromStr for $name {
            type Err = InvalidId;

            fn from_str(text: &str) -> Result<Self, Self::Err> {
                let wrong = InvalidId::Malformed { kind: $kind, prefix: $prefix };
                let hex = text
                    .strip_prefix(Self::PREFIX)
                    .and_then(|rest| rest.strip_prefix('_'))
                    .ok_or(wrong)?
                    .as_bytes();
                if hex.len() != 32 {
                    return Err(wrong);
                }
                let mut out = [0u8; 16];
                let (pairs, _) = hex.as_chunks::<2>();
                for (place, [high, low]) in out.iter_mut().zip(pairs) {
                    let high = nibble(*high).ok_or(wrong)?;
                    let low = nibble(*low).ok_or(wrong)?;
                    *place = (high << 4) | low;
                }
                Ok(Self(out))
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.collect_str(self)
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let text = <String as serde::Deserialize>::deserialize(deserializer)?;
                text.parse().map_err(serde::de::Error::custom)
            }
        }
    };
}

opaque_id! {
    /// One run: one call of a tool, from admission to its final state.
    pub struct RunId: "run", "r";
}

opaque_id! {
    /// One job: a piece of work a runner claims under a lease.
    pub struct JobId: "job", "j";
}

opaque_id! {
    /// One session: the signed-in state of one identity on one target in one
    /// environment.
    pub struct SessionId: "session", "s";
}

opaque_id! {
    /// One approval a person gave.
    pub struct ApprovalId: "approval", "a";
}

opaque_id! {
    /// One recorded policy decision.
    pub struct DecisionId: "decision", "d";
}

opaque_id! {
    /// One workspace: the boundary nothing crosses.
    pub struct WorkspaceId: "workspace", "w";
}

opaque_id! {
    /// One principal: a person, an agent, or a program that calls.
    pub struct PrincipalId: "principal", "p";
}

opaque_id! {
    /// One runner: a process that claims jobs and runs the engine.
    pub struct RunnerId: "runner", "n";
}

/// A lease number. It only ever increases for a run, and a call that carries
/// a lower one than the current is refused, so a runner that lost its lease
/// can change nothing.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Default,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(transparent)]
pub struct Epoch(u64);

impl Epoch {
    /// A lease number from its value.
    #[must_use]
    pub const fn new(number: u64) -> Self {
        Self(number)
    }

    /// The number.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// The next lease number, or `None` when the numbers have run out.
    #[must_use]
    pub const fn next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(number) => Some(Self(number)),
            None => None,
        }
    }
}

impl fmt::Display for Epoch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Who a system sees when a tool acts on it: a person, as themself, or a
/// dedicated account. It names who; it is never a credential.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Identity {
    /// A person, signed in as themself. Only that person, or an agent acting
    /// for them, may use their session.
    Person(PrincipalId),
    /// A dedicated account, named in the settings.
    Account(AccountName),
}

impl fmt::Display for Identity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Person(person) => write!(f, "person:{person}"),
            Self::Account(account) => write!(f, "account:{account}"),
        }
    }
}

impl FromStr for Identity {
    type Err = InvalidId;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        match text.split_once(':') {
            Some(("person", rest)) => rest
                .parse()
                .map(Self::Person)
                .map_err(|_| InvalidId::IdentityMalformed),
            Some(("account", rest)) => AccountName::new(rest)
                .map(Self::Account)
                .map_err(|_| InvalidId::IdentityMalformed),
            _ => Err(InvalidId::IdentityMalformed),
        }
    }
}

impl serde::Serialize for Identity {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for Identity {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = <String as serde::Deserialize>::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

/// A version of the engine, the compiler, or a target: three whole numbers,
/// ordered as written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    /// Raised by a change that breaks what came before.
    pub major: u32,
    /// Raised by an addition.
    pub minor: u32,
    /// Raised by a correction.
    pub patch: u32,
}

impl Version {
    /// A version from its three numbers.
    #[must_use]
    pub const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl FromStr for Version {
    type Err = InvalidId;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let number = |part: &str| {
            let plain = !part.is_empty()
                && part.len() <= 10
                && part.bytes().all(|b| b.is_ascii_digit())
                && (part == "0" || !part.starts_with('0'));
            if plain {
                part.parse::<u32>().ok()
            } else {
                None
            }
        };
        let mut parts = text.split('.');
        let version = (|| {
            let version = Self::new(
                number(parts.next()?)?,
                number(parts.next()?)?,
                number(parts.next()?)?,
            );
            parts.next().is_none().then_some(version)
        })();
        version.ok_or(InvalidId::VersionMalformed)
    }
}

impl serde::Serialize for Version {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for Version {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = <String as serde::Deserialize>::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AccountName, ControlName, Environment, Epoch, FileKindName, Identity, InvalidId, JobId,
        LintCode, OperationName, PrincipalId, RunId, StepKind, SurfaceKind, SurfaceName,
        TargetName, Verb, Version,
    };
    use crate::{Failure, FailureClass, FailureCode, Timestamp};

    #[test]
    fn every_checked_name_refuses_a_badly_formed_one() {
        fn refuses<T, E>(make: impl Fn(&str) -> Result<T, E>, longest: usize)
        where
            E: Into<Failure>,
        {
            assert!(make("example").is_ok());
            assert!(make(&"e".repeat(longest)).is_ok());
            for bad in ["", "Example", "an example", "ex-ample", "1st", "_x", "é"] {
                let Err(error) = make(bad) else {
                    panic!("`{bad}` was accepted")
                };
                let failure: Failure = error.into();
                assert_eq!(failure.class(), FailureClass::Input);
                assert_eq!(failure.code(), &FailureCode::SCHEMA_INVALID);
            }
            assert!(make(&"e".repeat(longest + 1)).is_err());
        }
        refuses(TargetName::new, TargetName::LONGEST);
        refuses(OperationName::new, OperationName::LONGEST);
        refuses(SurfaceName::new, SurfaceName::LONGEST);
        refuses(Environment::new, Environment::LONGEST);
        refuses(SurfaceKind::new, SurfaceKind::LONGEST);
        refuses(StepKind::new, StepKind::LONGEST);
        refuses(Verb::new, Verb::LONGEST);
        refuses(FileKindName::new, FileKindName::LONGEST);
        refuses(ControlName::new, ControlName::LONGEST);
        refuses(AccountName::new, AccountName::LONGEST);
    }

    #[test]
    fn a_checked_name_is_read_and_written_as_its_text_and_says_nothing_of_registration() {
        let kind = StepKind::new("locate").unwrap();
        assert_eq!(serde_json::to_string(&kind).unwrap(), "\"locate\"");
        assert_eq!(
            serde_json::from_str::<StepKind>("\"locate\"").unwrap(),
            kind
        );
        assert!(serde_json::from_str::<StepKind>("\"Locate It\"").is_err());
        assert_eq!("upsert".parse::<Verb>().unwrap().to_string(), "upsert");
    }

    #[test]
    fn a_refused_name_never_echoes_more_than_64_characters() {
        let long = "X".repeat(5_000);
        let text = TargetName::new(&long).unwrap_err().to_string();
        assert!(text.len() < 400, "{}", text.len());
        let text = LintCode::new(&long).unwrap_err().to_string();
        assert!(text.len() < 500, "{}", text.len());
    }

    #[test]
    fn an_operation_name_belongs_to_the_target_it_starts_with() {
        let target = TargetName::new("example").unwrap();
        let name = OperationName::new("example_orders_order_read").unwrap();
        assert!(name.belongs_to(&target));
        for other in ["examples_order_read", "example", "other_example_order_read"] {
            assert!(
                !OperationName::new(other).unwrap().belongs_to(&target),
                "{other}"
            );
        }
    }

    #[test]
    fn a_lint_code_is_an_owner_and_a_rule_joined_by_one_dot() {
        let code = LintCode::new("kernel.no_fixed_wait").unwrap();
        assert_eq!(code.owner(), "kernel");
        assert_eq!(code.rule(), "no_fixed_wait");
        assert_eq!(
            serde_json::from_str::<LintCode>("\"kernel.no_fixed_wait\"").unwrap(),
            code
        );
        for bad in [
            "",
            "kernel",
            "kernel.",
            ".rule",
            "kernel.no.wait",
            "Kernel.rule",
            "kernel.Rule",
            "kernel rule",
            &format!("kernel.{}", "r".repeat(60)),
        ] {
            let failure: Failure = LintCode::new(bad).unwrap_err().into();
            assert_eq!(failure.code(), &FailureCode::SCHEMA_INVALID, "{bad}");
        }
    }

    #[test]
    fn an_identifier_is_made_from_a_moment_and_random_bytes_and_reads_back() {
        let at = Timestamp::from_unix_millis(1_795_000_000_123);
        let run = RunId::from_parts(at, [0xff; 10]).unwrap();
        assert_eq!(run.made_at(), at);
        assert_eq!(run, RunId::from_parts(at, [0xff; 10]).unwrap());
        let text = run.to_string();
        assert!(text.starts_with("r_"), "{text}");
        assert_eq!(text.len(), 34);
        assert_eq!(text.parse::<RunId>().unwrap(), run);
        assert_eq!(RunId::from_bytes(*run.as_bytes()), run);
        assert_eq!(format!("{run:?}"), format!("RunId({text})"));
        let json = serde_json::to_string(&run).unwrap();
        assert_eq!(json, format!("\"{text}\""));
        assert_eq!(serde_json::from_str::<RunId>(&json).unwrap(), run);
        // The marks a store's time-ordered identifier type expects.
        assert_eq!(run.as_bytes()[6] >> 4, 7);
        assert_eq!(run.as_bytes()[8] >> 6, 0b10);
    }

    #[test]
    fn identifiers_made_later_sort_later() {
        let early = RunId::from_parts(Timestamp::from_unix_millis(1_000), [0xff; 10]).unwrap();
        let late = RunId::from_parts(Timestamp::from_unix_millis(1_001), [0x00; 10]).unwrap();
        assert!(early < late);
    }

    #[test]
    fn text_that_is_not_an_identifier_is_refused() {
        let run = RunId::from_parts(Timestamp::from_unix_millis(5), [7; 10]).unwrap();
        let text = run.to_string();
        let upper = text.to_uppercase();
        let short = &text[..33];
        let other_kind = text.replacen("r_", "j_", 1);
        for bad in ["", "r_", short, upper.as_str(), other_kind.as_str(), "r_zz"] {
            let failure: Failure = bad.parse::<RunId>().unwrap_err().into();
            assert_eq!(failure.code(), &FailureCode::SCHEMA_INVALID, "{bad}");
        }
        assert!(other_kind.parse::<JobId>().is_ok());
    }

    #[test]
    fn a_moment_an_identifier_cannot_hold_is_refused() {
        for millis in [-1, 1 << 48] {
            let error = RunId::from_parts(Timestamp::from_unix_millis(millis), [0; 10]);
            assert_eq!(error, Err(InvalidId::MomentOutOfRange));
        }
        let failure: Failure = InvalidId::MomentOutOfRange.into();
        assert_eq!(failure.code(), &FailureCode::VALUE_OUT_OF_RANGE);
    }

    #[test]
    fn an_identity_is_a_person_or_an_account_and_never_a_bare_string() {
        let person = PrincipalId::from_parts(Timestamp::from_unix_millis(9), [1; 10]).unwrap();
        let as_person = Identity::Person(person);
        let as_account = Identity::Account(AccountName::new("order_desk").unwrap());
        assert_eq!(as_account.to_string(), "account:order_desk");
        for identity in [as_person, as_account] {
            let text = identity.to_string();
            assert_eq!(text.parse::<Identity>().unwrap(), identity);
            let json = serde_json::to_string(&identity).unwrap();
            assert_eq!(serde_json::from_str::<Identity>(&json).unwrap(), identity);
        }
        for bad in [
            "",
            "order_desk",
            "account:",
            "account:Order Desk",
            "person:x",
            "role:admin",
        ] {
            assert!(bad.parse::<Identity>().is_err(), "{bad}");
        }
    }

    #[test]
    fn a_version_is_three_numbers_ordered_as_numbers() {
        let version: Version = "1.10.0".parse().unwrap();
        assert_eq!(version, Version::new(1, 10, 0));
        assert!(version > Version::new(1, 9, 7));
        assert_eq!(version.to_string(), "1.10.0");
        assert_eq!(serde_json::to_string(&version).unwrap(), "\"1.10.0\"");
        for bad in [
            "",
            "1",
            "1.2",
            "1.2.3.4",
            "1.02.3",
            "1.-2.3",
            "1.2.x",
            "+1.2.3",
            "1.2.99999999999",
        ] {
            assert!(bad.parse::<Version>().is_err(), "{bad}");
        }
    }

    #[test]
    fn a_lease_number_only_goes_up() {
        let first = Epoch::default();
        let second = first.next().unwrap();
        assert!(second > first);
        assert_eq!(second.get(), 1);
        assert_eq!(Epoch::new(u64::MAX).next(), None);
    }
}
