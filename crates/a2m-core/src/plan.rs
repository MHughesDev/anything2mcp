//! The compiled plan, as data: what the compiler emits and the engine runs.
//!
//! Nothing here builds a plan from target files, evaluates an expression, or
//! decodes a pack's payload. This module holds the plan's shape, its one
//! encoding, and the check of its hash. No type in it holds a float, a
//! timestamp, a path from a developer's machine, or a map with no order of
//! its own.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::canonical::{self, CanonicalJson, PackPayload};
use crate::decimal::{Decimal, Rounding};
use crate::digest::Digest;
use crate::effect::{EffectClass, Hazard};
use crate::evidence::WriteEvidence;
use crate::failure::{Failure, FailureClass, FailureCode};
use crate::ident::{
    AuthProviderName, CredentialRef, Environment, FileKindName, OperationName, StepKind,
    SurfaceKind, SurfaceName, TargetName, Verb, Version,
};
use crate::kind::Mode;
use crate::state::BindingStatus;
use crate::value::{RecordKey, Value, ValueType, Values};
use crate::vocabulary::vocabulary;

/// The plan format this crate writes. The engine loads this one and, once
/// there is one, the one before it.
pub const PLAN_FORMAT: u16 = 1;

/// The formats `Plan::decode` reads.
const READABLE_FORMATS: &[u16] = &[PLAN_FORMAT];

/// What every encoded plan starts with, before its format number and a line
/// end.
const ENVELOPE: &[u8] = b"anything2mcp-plan ";

vocabulary! {
    /// What a field type is built on (file formats, `types.yaml`).
    pub enum FieldBase: "field type base" {
        /// Text.
        Text => "text",
        /// A whole number.
        Integer => "integer",
        /// An exact number.
        Decimal => "decimal",
        /// An amount in a currency.
        Money => "money",
        /// A quantity in a unit.
        Measurement => "measurement",
        /// A calendar day.
        Date => "date",
        /// A day and a time.
        Datetime => "datetime",
        /// A time of day.
        Time => "time",
        /// True or false.
        Boolean => "boolean",
        /// One of a list of stored codes.
        Choice => "choice",
    }
}

vocabulary! {
    /// How carefully a field's values are handled (file formats,
    /// `types.yaml`). The order is from least to most careful.
    #[derive(PartialOrd, Ord, Default)]
    pub enum Sensitivity: "sensitivity" {
        /// May be shown anywhere.
        Public => "public",
        /// Kept inside the organization. What a field with no label is.
        #[default]
        Internal => "internal",
        /// Personal or restricted data: kept as a digest in traces.
        Restricted => "restricted",
    }
}

vocabulary! {
    /// How a write's record is found again (spec section 209).
    pub enum IdentityStrategy: "identity strategy" {
        /// By an input that is the system's own key.
        Natural => "natural",
        /// By several inputs that are unique together, within a parent.
        Composite => "composite",
        /// By a token written into a field of the record.
        Stamped => "stamped",
        /// By a declared lookup that must match exactly one record.
        Find => "find",
        /// It cannot be; the operation is irreversible.
        None => "none",
    }
}

vocabulary! {
    /// Which kind of copy of a system an environment is (file formats,
    /// "Environment").
    pub enum EnvironmentKind: "environment kind" {
        /// A copy that is safe to write to.
        Test => "test",
        /// The system people rely on.
        Production => "production",
    }
}

vocabulary! {
    /// How a test of a tool runs (file formats, `tests/`).
    pub enum TestKind: "test kind" {
        /// Against a fixture, with no system.
        Offline => "offline",
        /// Against a test record in a system.
        Live => "live",
    }
}

vocabulary! {
    /// Where a pipeline step takes a snapshot as evidence (file formats,
    /// "Step").
    pub enum Checkpoint: "checkpoint" {
        /// After every record.
        PerRecord => "per_record",
        /// After the step.
        PerStep => "per_step",
    }
}

vocabulary! {
    /// How a sign-in comes to exist (spec section 215).
    pub enum SignInMode: "sign-in mode" {
        /// What the device already has.
        UseExisting => "use_existing",
        /// A person signs in once, in a place the engine owns.
        ManagedSession => "managed_session",
        /// Dedicated credentials, held in the vault.
        Custom => "custom",
    }
}

vocabulary! {
    /// How often a system changes under its tools (file formats,
    /// `target.yaml`). It sets how often drift is checked. The order is from
    /// least to most often.
    #[derive(PartialOrd, Ord)]
    pub enum DriftRisk: "drift risk" {
        /// It seldom changes.
        Low => "low",
        /// It changes now and then.
        Medium => "medium",
        /// It changes often.
        High => "high",
    }
}

vocabulary! {
    /// How much evidence a run keeps (spec section 217). Each level includes
    /// the one before it.
    #[derive(PartialOrd, Ord)]
    pub enum CaptureLevel: "capture level" {
        /// Run events and the step trace.
        Minimal => "minimal",
        /// Plus a snapshot at each checkpoint and at any failure.
        Standard => "standard",
        /// Plus every exchange and a screenshot per step.
        Full => "full",
    }
}

vocabulary! {
    /// Who must approve a write (spec sections 212 and 226). The order is
    /// from least to most strict.
    #[derive(PartialOrd, Ord)]
    pub enum ApproveWrites: "approval rule" {
        /// The caller confirms its own write.
        CallerConfirms => "caller_confirms",
        /// A person approves every run.
        PersonEachRun => "person_each_run",
        /// A person other than the one whose call started the run.
        NamedApprover => "named_approver",
    }
}

vocabulary! {
    /// What is done to whitespace before text is compared (file formats,
    /// `types.yaml`).
    pub enum WhitespaceRule: "whitespace rule" {
        /// Every run of whitespace becomes one space, and the ends have none.
        Collapse => "collapse",
        /// Whitespace at the ends is dropped.
        Trim => "trim",
        /// Text is compared as written.
        Exact => "exact",
    }
}

vocabulary! {
    /// The mark between the whole part of a number and its fraction, as a
    /// system shows it (file formats, `types.yaml`).
    pub enum DecimalMark: "decimal mark" {
        /// A point, with a comma between groups of digits: `1,234.50`.
        Point => ".",
        /// A comma, with a point between groups of digits: `1.234,50`.
        Comma => ",",
    }
}

/// Defines the position of one part of a plan in its list.
macro_rules! plan_index {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(u32);

        impl $name {
            /// The index of position `position`.
            #[must_use]
            pub const fn new(position: u32) -> Self {
                Self(position)
            }

            /// The position, for indexing the list.
            #[must_use]
            pub const fn get(self) -> usize {
                self.0 as usize
            }
        }
    };
}

plan_index! {
    /// Which of a plan's expressions.
    ExprIndex
}
plan_index! {
    /// Which of a plan's surfaces.
    SurfaceIndex
}
plan_index! {
    /// Which of a plan's environments.
    EnvironmentIndex
}
plan_index! {
    /// Which of a plan's field types.
    FieldTypeIndex
}
plan_index! {
    /// Which of an operation's inputs.
    InputIndex
}
plan_index! {
    /// Which of a plan's operations.
    OperationIndex
}
plan_index! {
    /// Which of a plan's pipelines.
    PipelineIndex
}
plan_index! {
    /// Which of a plan's pack sections.
    SectionIndex
}
plan_index! {
    /// Which of a plan's tools.
    ToolIndex
}

/// One target version, compiled: everything the engine needs to run its
/// tools, and nothing it has to parse, resolve, or check at call time.
///
/// A plan is immutable data. The compiler is its only producer; the engine
/// receives one only through [`Plan::decode`], so it never runs bytes whose
/// hash it has not checked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    /// What the plan is, and what made it.
    pub header: PlanHeader,
    /// What holds for the target as a whole.
    pub target: TargetPlan,
    /// The target's surfaces.
    pub surfaces: Vec<SurfacePlan>,
    /// The target's environments, with the addresses each one changes.
    pub environments: Vec<EnvironmentPlan>,
    /// How each group of surfaces is signed in to.
    pub auth: Vec<AuthPlan>,
    /// How each kind of value is normalized, compared, and classified.
    pub field_types: Vec<FieldTypePlan>,
    /// Every expression, once. Everything else refers to one by its index.
    pub expressions: Vec<PlanExpr>,
    /// The operations.
    pub operations: Vec<OperationPlan>,
    /// The pipelines.
    pub pipelines: Vec<PipelinePlan>,
    /// The tools, as the front doors list them.
    pub tools: Vec<ToolPlan>,
    /// Each pack's own compiled knowledge, opaque here.
    pub packs: Vec<PackSection>,
    /// Which tests belong to which tool.
    pub tests: Vec<TestPlan>,
}

/// What a plan is, and what made it. It holds no timestamp and no path, so
/// the same sources give the same plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanHeader {
    /// The plan format. [`PLAN_FORMAT`] for a plan written by this crate.
    pub format: u16,
    /// The oldest engine that may run the plan. The engine checks it.
    pub minimum_engine: Version,
    /// The compiler that made the plan.
    pub compiler: Version,
    /// The target.
    pub target: TargetName,
    /// The target's version.
    pub version: Version,
    /// The digest of the sorted list of source paths and file digests.
    pub sources: Digest,
}

/// What `target.yaml` states for the target as a whole. A published version
/// pins every part of it.
///
/// What belongs to one kind of surface, such as a viewport, is not here: it
/// is a field of that surface, in the surface's payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetPlan {
    /// The name shown to people.
    pub title: String,
    /// The target's modules, in declared order. Empty for a target that has
    /// none.
    pub modules: Vec<String>,
    /// The environment a call runs in when it names none.
    pub default_environment: EnvironmentIndex,
    /// The locale every run sees, such as `en-US`, when the target fixes one.
    pub locale: Option<String>,
    /// The time zone every run sees, when the target fixes one: `UTC` or an
    /// offset such as `+02:00`.
    pub timezone: Option<String>,
    /// The capture level of a run, when the target states one. Policy may
    /// raise it.
    pub capture: Option<CaptureLevel>,
    /// True when the system allows one session per account, so that signing
    /// in from the engine signs the person out elsewhere.
    pub exclusive_session: bool,
    /// How often the system changes under its tools, when the target says.
    pub drift_risk: Option<DriftRisk>,
    /// A reference to the record of who agreed to this target being wrapped.
    /// It names a record; it holds nothing of it.
    pub authorization: Option<String>,
}

/// Where a surface stands to an application that owns its data (spec
/// section 401).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stands {
    /// It is the system of record.
    Alone,
    /// It lies beneath the application reached through this surface.
    Beneath(SurfaceIndex),
}

/// One surface of the target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfacePlan {
    /// Its name, unique within the target.
    pub name: SurfaceName,
    /// Its kind, which a pack registered.
    pub kind: SurfaceKind,
    /// Where it is reached, in the form its kind defines.
    pub address: String,
    /// The surface whose sign-in this one shares.
    pub same_session_as: Option<SurfaceIndex>,
    /// Where it stands to an application that owns its data. Nothing when
    /// the target does not say, which a pack may treat as unknown.
    pub stands: Option<Stands>,
    /// How many requests it accepts in a period.
    pub rate: Option<RatePlan>,
    /// The programs its driver may start, in any environment of the target.
    /// Empty for a surface that is not reached by starting a program. The
    /// engine hands them to the supervisor and reads nothing else in them.
    pub programs: Vec<DeclaredProgram>,
    /// What else its pack compiled about it.
    pub payload: PackPayload,
}

/// A program a surface's driver may start, as the target's files declare it.
/// The supervisor starts no program for the surface's driver that is not
/// declared, by the pack's manifest or here (spec section 204).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeclaredProgram {
    /// Its absolute path, as the target's file wrote it. Never a name to
    /// search for. It is text, so that a plan compiled on one system can
    /// name a program of another.
    pub path: String,
    /// The digest the file must have, when the target states one.
    pub digest: Option<Digest>,
}

impl DeclaredProgram {
    /// Whether `path` is absolute on some system this product runs on: it
    /// starts at the root (`/opt/example/bin/tool`), at a drive
    /// (`C:\example\tool.exe`, with either slash), or at a network share
    /// (`\\server\share\tool.exe`). The answer is the same on every
    /// machine, so the same files give the same plan. A path that holds a
    /// NUL character is never one.
    #[must_use]
    pub fn is_absolute_path(path: &str) -> bool {
        let bytes = path.as_bytes();
        if bytes.contains(&0) {
            return false;
        }
        match bytes {
            [b'/', ..] => true,
            [drive, b':', b'/' | b'\\', ..] => drive.is_ascii_alphabetic(),
            [b'\\', b'\\', rest @ ..] => rest.first().is_some_and(|byte| *byte != b'\\'),
            _ => false,
        }
    }

    /// Whether `path` names its file directly: no part of it is `.` or
    /// `..`, with either slash. A path that steps back is read differently
    /// by a person and by the system that follows it, so a plan holds none.
    #[must_use]
    pub fn is_direct_path(path: &str) -> bool {
        !path
            .split(['/', '\\'])
            .any(|part| part == "." || part == "..")
    }

    /// Whether `path` is on a network share (`\\server\share\tool.exe`).
    /// The file behind such a path belongs to another machine, so a plan
    /// names one only with the digest its bytes must have.
    #[must_use]
    pub fn is_share_path(path: &str) -> bool {
        path.starts_with("\\\\") && Self::is_absolute_path(path)
    }
}

/// How many requests a surface accepts in a period.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RatePlan {
    /// The number of requests.
    pub requests: u32,
    /// The length of the period, in milliseconds.
    pub per_ms: u32,
}

/// One environment of the target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentPlan {
    /// Its name.
    pub name: Environment,
    /// Whether it is a test copy or the system people rely on.
    pub kind: EnvironmentKind,
    /// The surfaces it reaches at another address.
    pub addresses: Vec<AddressOverride>,
    /// The record keys that drafts and publish gates may write to, each as
    /// its target file wrote it. Policy compares a run's record key with
    /// these.
    pub test_records: Vec<RecordKey>,
    /// The approval rule of this environment, when the target states one.
    /// Policy applies whichever of this and the installation's own rule is
    /// the stricter, so a target can tighten the rule and never loosen it.
    pub approve_writes: Option<ApproveWrites>,
}

/// The address of one surface in one environment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddressOverride {
    /// Which surface.
    pub surface: SurfaceIndex,
    /// Where it is reached in this environment.
    pub address: String,
}

/// What a session is checked with: one cheap read that works only while the
/// sign-in holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbePlan {
    /// A read operation of the target, run through the engine as any read
    /// is, with these inputs.
    Operation {
        /// The operation. Its effect is `read`.
        operation: OperationIndex,
        /// The inputs it is called with.
        inputs: Values,
    },
    /// An entry of a pack section, such as an endpoint, which the pack of
    /// the surface reads.
    Entry(PackRef),
}

/// One setting of an auth provider: a name the provider documents, and a
/// plain value. Never a credential.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthSetting {
    /// The setting's name.
    pub name: String,
    /// Its value, as text.
    pub value: String,
}

/// One stored credential an auth provider uses, by the role the provider
/// gives it. The plan holds the reference; the value never leaves the vault
/// except to the provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CredentialUse {
    /// The role, such as `password` or `client_secret`.
    pub role: String,
    /// The name the vault resolves.
    pub reference: CredentialRef,
}

/// How one group of surfaces that share a sign-in is signed in to: one entry
/// of `auth.yaml`. The session keeper reads the typed fields; the provider
/// reads its settings and credentials; the expiry signals are read by the
/// pack that owns the surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthPlan {
    /// The surfaces the entry covers. No surface is covered twice.
    pub surfaces: Vec<SurfaceIndex>,
    /// How the sign-in comes to exist.
    pub mode: SignInMode,
    /// The auth provider that signs in.
    pub provider: AuthProviderName,
    /// The read that shows the sign-in still holds.
    pub probe: ProbePlan,
    /// The signs that the session has ended, in declared order, each in the
    /// form the surface's pack defines. Opaque here.
    pub expiry_signals: Vec<PackPayload>,
    /// The system's idle timeout, in milliseconds, when the target states it.
    pub idle_ms: Option<u64>,
    /// The longest a sign-in lasts, in milliseconds, when the target states
    /// it.
    pub absolute_ms: Option<u64>,
    /// What ends the session on the system's side: a name the provider
    /// resolves, such as an endpoint or an action.
    pub sign_out: Option<String>,
    /// The provider's own settings, sorted by name.
    pub settings: Vec<AuthSetting>,
    /// The stored credentials the provider uses, sorted by role. References
    /// only.
    pub credentials: Vec<CredentialUse>,
    /// How many times a stored credential is tried without a person.
    pub attempt_limit: u32,
    /// Whether the session's state may be saved and restored on another
    /// runner.
    pub movable: bool,
}

/// Which zone a date or a time is read in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZoneRule {
    /// The one zone the target's settings fix.
    Fixed,
    /// The zone of the person the system sees.
    User,
    /// A named zone, such as one from the zone database.
    Named(String),
}

/// One stored code of a choice, and what people see for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodeLabel {
    /// The code the system stores. Choices are compared by this.
    pub code: String,
    /// The text shown for it.
    pub label: String,
}

/// How a system shows a value of one field type, and so how a value is
/// written for it and described to a caller. It is data beside the
/// normalizer and says the same thing; the normalizer alone decides whether
/// two values are equal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldFormat {
    /// Text.
    Text {
        /// False when case is ignored in comparison.
        case_sensitive: bool,
        /// What is done to whitespace.
        whitespace: WhitespaceRule,
    },
    /// A whole number, a decimal, money, or a measurement. Its places,
    /// rounding, step, and units are the field type's own fields.
    Number {
        /// The mark before the fraction.
        decimal_mark: DecimalMark,
    },
    /// A date, a time, or a date and a time.
    Calendar {
        /// The pattern the system shows and accepts, such as `MM/DD/YYYY`.
        pattern: String,
        /// For a date and a time whose zone the compiler could fix: the
        /// zone's offset from UTC in minutes, east positive.
        utc_offset_minutes: Option<i32>,
    },
    /// True or false.
    Boolean {
        /// The text shown for true.
        true_text: String,
        /// The text shown for false.
        false_text: String,
    },
    /// One of the field type's codes, shown by its label.
    Choice,
}

/// How one kind of value is formatted, normalized, compared, and classified.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldTypePlan {
    /// The field type's name, such as `money` or one the target adds.
    pub name: String,
    /// What it is built on.
    pub base: FieldBase,
    /// The type of a normalized value.
    pub ty: ValueType,
    /// Decimal places, for numbers.
    pub places: Option<u32>,
    /// How a number is rounded to those places.
    pub rounding: Option<Rounding>,
    /// The smallest increment of a measurement, in the unit it is stored in.
    pub step: Option<Decimal>,
    /// The unit a measurement is stored in.
    pub unit: Option<String>,
    /// The unit a measurement is shown in.
    pub display_unit: Option<String>,
    /// The currency, for money.
    pub currency: Option<String>,
    /// The zone rule, for dates and times.
    pub zone: Option<ZoneRule>,
    /// The stored codes and their labels, for choices, in declared order.
    pub codes: Vec<CodeLabel>,
    /// How carefully values of this type are handled.
    pub sensitivity: Sensitivity,
    /// True for free text that came from the system.
    pub untrusted: bool,
    /// How the system shows a value, for writing one and for describing the
    /// type.
    pub format: FieldFormat,
    /// The expression that normalizes a raw value, as host-function calls.
    /// The one rule by which two values are compared.
    pub normalizer: Option<ExprIndex>,
}

/// One expression: canonical source text that the compiler has already
/// checked, the type of its result, and the version of the host-function
/// library it was checked against. This crate never parses one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanExpr {
    /// The canonical text: the parsed program printed back.
    pub source: String,
    /// The type of the result.
    pub result: ValueType,
    /// The host-function library version.
    pub library: u32,
}

/// One input of an operation or a pipeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputPlan {
    /// The input's name.
    pub name: String,
    /// Its field type.
    pub field_type: FieldTypeIndex,
    /// The type of its value.
    pub ty: ValueType,
    /// Whether a caller must give it.
    pub required: bool,
    /// The value used when a caller gives none.
    pub default: Option<Value>,
    /// What it means, for the caller.
    pub description: String,
}

/// One field of what an operation returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputPlan {
    /// The field's name.
    pub name: String,
    /// The type of its value.
    pub ty: ValueType,
    /// Its field type, where it has one.
    pub field_type: Option<FieldTypeIndex>,
}

/// How an operation's record is found again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentityPlan {
    /// The strategy.
    pub strategy: IdentityStrategy,
    /// The inputs whose normalized values are the record key, in declared
    /// order, parent first.
    pub key: Vec<InputIndex>,
}

/// What an operation may touch beyond its bindings' steps.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionManifest {
    /// The surfaces it may reach.
    pub surfaces: Vec<SurfaceIndex>,
    /// What it may send as a file, by the name of the root or the input.
    pub uploads: Vec<String>,
    /// What it may receive as a file.
    pub downloads: Vec<String>,
    /// Addresses beyond its surfaces that it may reach.
    pub destinations: Vec<String>,
}

/// A question a person may have to answer during a run. A run asks only
/// what its operation declares.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionPoint {
    /// The decision point's name, unique within the operation.
    pub name: String,
    /// The question, as the person is asked it.
    pub question: String,
}

/// One operation: one thing a tool does to the system.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationPlan {
    /// Its name, which is its tool's name.
    pub name: OperationName,
    /// Whether it reads, writes, or cannot be undone.
    pub effect: EffectClass,
    /// What it can reach beyond the record.
    pub hazard: Hazard,
    /// Its inputs, in declared order.
    pub inputs: Vec<InputPlan>,
    /// The fields of its result, in declared order.
    pub outputs: Vec<OutputPlan>,
    /// How its record is found again.
    pub identity: IdentityPlan,
    /// How many times the system saves, counted from the lowered steps.
    pub commit_points: u32,
    /// Its bindings, in the order the engine tries them.
    pub bindings: Vec<BindingPlan>,
    /// What it may touch.
    pub manifest: PermissionManifest,
    /// Example inputs, for the caller: each a value of the operation's
    /// inputs.
    pub examples: Vec<Values>,
    /// The questions a person may have to answer during a run.
    pub decision_points: Vec<DecisionPoint>,
}

/// One way of carrying out an operation, through one mode.
///
/// The engine treats a binding by what its steps and its operation state:
/// which steps commit, which is a lookup, the operation's effect, and its
/// identity strategy. The verb is kept for traces and messages and is never
/// read for its spelling (decision 0068).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BindingPlan {
    /// The mode whose driver carries out the steps.
    pub mode: Mode,
    /// What the binding does, as its file named it.
    pub verb: Verb,
    /// How far the binding has come.
    pub status: BindingStatus,
    /// The surface it acts on.
    pub surface: SurfaceIndex,
    /// The steps that do the work.
    pub act: Vec<Step>,
    /// The steps that see the system accept it.
    pub confirm: Vec<Step>,
    /// The steps that read the record: after a write, and on their own for
    /// the preview, the freshness check, and reconcile.
    pub read_back: Vec<Step>,
    /// The steps that find the record by its stamp or by a declared search,
    /// for the `stamped` and `find` strategies.
    pub lookup: Vec<Step>,
    /// How long a lookup may lag behind a write, in milliseconds.
    pub settle_ms: u32,
    /// What stands as the evidence that a write landed: the read-back, the
    /// save's own answer, or, for an irreversible operation with nothing to
    /// read, the committing step's acknowledgement alone. A lowering's
    /// `prefer_response` compiles to `response`.
    pub evidence: WriteEvidence,
    /// What must hold before the binding may be chosen.
    pub precondition: Option<ExprIndex>,
}

/// One fixed instruction, carried out by the driver of its binding's mode.
/// A step holds no logic: it is done as written, or it fails as it says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    /// What kind of step, a name within the binding's mode.
    pub kind: StepKind,
    /// What it refers to inside a pack section: a screen field, an action,
    /// an endpoint.
    pub refers_to: Option<PackRef>,
    /// Its arguments. The engine evaluates each and hands the driver the
    /// values by name.
    pub args: Vec<StepArg>,
    /// How long it may take, in milliseconds.
    pub timeout_ms: u32,
    /// The failure to raise when it cannot be done.
    pub fails_as: FailsAs,
    /// Whether it is a save in the system: one row in the ledger. A step
    /// that commits needs a recorded intent.
    pub commits: bool,
    /// Whether it looks a record up, and so may report it absent.
    pub lookup: bool,
    /// Whether sending it twice does what sending it once does, because the
    /// system takes the idempotency token.
    pub idempotent: bool,
    /// The step is skipped when this is false.
    pub when: Option<ExprIndex>,
    /// The pack's own details for the step.
    pub payload: PackPayload,
}

/// One named argument of a step, of a pipeline step, or of a source mapping.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepArg {
    /// The argument's name.
    pub name: String,
    /// The expression that gives its value.
    pub value: ExprIndex,
}

/// A reference to one entry of a pack section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackRef {
    /// Which pack section.
    pub section: SectionIndex,
    /// The entry's name inside it, as the pack defines names.
    pub entry: String,
}

/// The failure a step raises when it cannot be done as written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FailsAs {
    /// The class.
    pub class: FailureClass,
    /// The code within the class.
    pub code: FailureCode,
}

/// What a pipeline may not exceed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineLimits {
    /// The most records one run may touch.
    pub max_records: Option<u32>,
    /// The longest one run may take, in milliseconds.
    pub max_duration_ms: Option<u64>,
}

/// One step of a pipeline: one operation, called with inputs worked out from
/// what came before.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineStep {
    /// The step's name within the pipeline.
    pub id: String,
    /// The operation it calls.
    pub operation: OperationIndex,
    /// The operation's inputs, each from an expression.
    pub with: Vec<StepArg>,
    /// The rows to repeat over, in order.
    pub for_each: Option<ExprIndex>,
    /// Where a snapshot is taken as evidence.
    pub checkpoint: Option<Checkpoint>,
    /// The step is skipped when this is false.
    pub when: Option<ExprIndex>,
    /// What must be true of the result, or the run stops.
    pub expect: Option<ExprIndex>,
}

/// Where a pipeline's rows come from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourcePlan {
    /// The source template's name and version, as the file states them.
    pub template: String,
    /// The reader that turns the input into rows.
    pub reader: String,
    /// The row fields that are worked out, each from an expression over the
    /// row's cells. A row field read from one column as it stands is not
    /// here: its column is named in the payload.
    pub mapping: Vec<StepArg>,
    /// Which rows are records.
    pub rows: Option<ExprIndex>,
    /// What else the reader needs: the fingerprint, where the data is.
    pub payload: PackPayload,
}

/// One pipeline: operations in order, over the rows of a source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelinePlan {
    /// Its name, which is its tool's name.
    pub name: OperationName,
    /// The highest effect class among its operations, or higher if declared.
    pub effect: EffectClass,
    /// The most serious hazard among its operations.
    pub hazard: Hazard,
    /// Its inputs, in declared order.
    pub inputs: Vec<InputPlan>,
    /// Where its rows come from.
    pub source: Option<SourcePlan>,
    /// What it may not exceed.
    pub limits: PipelineLimits,
    /// Its steps, in order.
    pub steps: Vec<PipelineStep>,
    /// What must hold for the run to be `completed`.
    pub success: Vec<ExprIndex>,
    /// The least session time that must be left before it starts, in
    /// milliseconds.
    pub headroom_ms: u32,
}

/// What a tool runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolRuns {
    /// One operation.
    Operation(OperationIndex),
    /// One pipeline.
    Pipeline(PipelineIndex),
}

/// What a tool tells a caller about itself before it is called.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolAnnotations {
    /// It changes nothing.
    pub read_only: bool,
    /// It can remove or overwrite something.
    pub destructive: bool,
    /// Calling it twice with the same input does what calling it once does.
    pub idempotent: bool,
    /// It reaches beyond the target's own records.
    pub open_world: bool,
}

/// One tool, as the front doors list it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolPlan {
    /// The tool's name.
    pub name: OperationName,
    /// What it runs.
    pub runs: ToolRuns,
    /// The module of the target it belongs to, where the target has modules.
    pub module: Option<String>,
    /// The name shown to people.
    pub title: String,
    /// What it does, when to prefer another tool, its side effects and
    /// limits.
    pub description: String,
    /// The JSON Schema of its input.
    pub input_schema: CanonicalJson,
    /// The JSON Schema of its result.
    pub output_schema: CanonicalJson,
    /// What it tells a caller about itself.
    pub annotations: ToolAnnotations,
}

/// What one pack's file kind compiled to. Stored and hashed here; decoded
/// only by the pack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackSection {
    /// The provider that owns the file kind.
    pub pack: String,
    /// The file kind.
    pub kind: FileKindName,
    /// The surface kind the file kind is registered for.
    pub surface_kind: SurfaceKind,
    /// The version of the payload's own shape, which the pack sets.
    pub format: u16,
    /// The compiled knowledge.
    pub payload: PackPayload,
}

/// One test of a tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TestPlan {
    /// The test's name.
    pub name: String,
    /// The tool under test.
    pub tool: ToolIndex,
    /// Whether it runs against a fixture or a system.
    pub kind: TestKind,
    /// The fixture folder, for an offline test.
    pub fixture: Option<String>,
    /// The input to call with.
    pub inputs: Values,
    /// The expected result, or the expected trace signature.
    pub expect: CanonicalJson,
    /// For a live write test: whether the original values are put back.
    pub restore: bool,
}

/// A plan as bytes, with the hash that names it.
#[derive(Clone, PartialEq, Eq)]
pub struct EncodedPlan {
    /// The envelope and the body.
    pub bytes: Vec<u8>,
    /// The SHA-256 digest of all of `bytes`.
    pub hash: Digest,
}

impl fmt::Debug for EncodedPlan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "EncodedPlan({} bytes, {})", self.bytes.len(), self.hash)
    }
}

/// A failure of the class `input` about a plan.
fn refused(code: FailureCode, message: impl Into<String>) -> Failure {
    let next_step = if code == FailureCode::PLAN_FORMAT_UNSUPPORTED {
        "Compile the target's stored sources again for this engine with `anything2mcp migrate`."
    } else {
        "Publish the target again, or make an earlier version current."
    };
    Failure::new(FailureClass::Input, code, message, next_step)
}

/// Refuses a reference to a part the plan does not have.
fn within(what: &'static str, position: usize, count: usize) -> Result<(), Failure> {
    if position < count {
        Ok(())
    } else {
        Err(refused(
            FailureCode::PLAN_UNREADABLE,
            format!("The plan refers to {what} {position} and has {count}."),
        ))
    }
}

impl Plan {
    /// The expression at `index`. Always present in a plan that came from
    /// [`Plan::decode`], which checks every reference.
    #[must_use]
    pub fn expression(&self, index: ExprIndex) -> Option<&PlanExpr> {
        self.expressions.get(index.get())
    }

    /// The surface at `index`.
    #[must_use]
    pub fn surface(&self, index: SurfaceIndex) -> Option<&SurfacePlan> {
        self.surfaces.get(index.get())
    }

    /// The environment at `index`.
    #[must_use]
    pub fn environment(&self, index: EnvironmentIndex) -> Option<&EnvironmentPlan> {
        self.environments.get(index.get())
    }

    /// The environment with this name.
    #[must_use]
    pub fn environment_named(&self, name: &Environment) -> Option<&EnvironmentPlan> {
        self.environments
            .iter()
            .find(|environment| &environment.name == name)
    }

    /// The environment a call runs in when it names none.
    #[must_use]
    pub fn default_environment(&self) -> Option<&EnvironmentPlan> {
        self.environment(self.target.default_environment)
    }

    /// How the surface at `index` is signed in to: the entry that covers it,
    /// or the entry of the surface whose sign-in it shares. Nothing for a
    /// surface that needs no sign-in.
    #[must_use]
    pub fn auth_of(&self, index: SurfaceIndex) -> Option<&AuthPlan> {
        let mut at = index;
        // A chain of shared sign-ins is no longer than the list of surfaces.
        for _ in 0..=self.surfaces.len() {
            if let Some(entry) = self.auth.iter().find(|entry| entry.surfaces.contains(&at)) {
                return Some(entry);
            }
            at = self.surface(at)?.same_session_as?;
        }
        None
    }

    /// The field type at `index`.
    #[must_use]
    pub fn field_type(&self, index: FieldTypeIndex) -> Option<&FieldTypePlan> {
        self.field_types.get(index.get())
    }

    /// The operation at `index`.
    #[must_use]
    pub fn operation(&self, index: OperationIndex) -> Option<&OperationPlan> {
        self.operations.get(index.get())
    }

    /// The pipeline at `index`.
    #[must_use]
    pub fn pipeline(&self, index: PipelineIndex) -> Option<&PipelinePlan> {
        self.pipelines.get(index.get())
    }

    /// The pack section at `index`.
    #[must_use]
    pub fn section(&self, index: SectionIndex) -> Option<&PackSection> {
        self.packs.get(index.get())
    }

    /// The tool with this name.
    #[must_use]
    pub fn tool(&self, name: &OperationName) -> Option<&ToolPlan> {
        self.tools.iter().find(|tool| &tool.name == name)
    }

    /// The pack section a pack compiled for one file kind and surface kind.
    #[must_use]
    pub fn section_of(
        &self,
        kind: &FileKindName,
        surface_kind: &SurfaceKind,
    ) -> Option<&PackSection> {
        self.packs
            .iter()
            .find(|section| &section.kind == kind && &section.surface_kind == surface_kind)
    }

    /// Checks that the plan holds together: its format is one this crate
    /// writes or reads, every reference from one part to another points at a
    /// part that exists, and every operation keeps the rules of a write: a
    /// read commits nowhere, a write has as many committing steps in each
    /// binding as it has commit points, at least one, only among the steps
    /// that do the work, and a read-back unless it is irreversible with no
    /// identity. `encode` and `decode` both call it, so neither the compiler
    /// nor the engine ever holds a plan with a loose reference or a write
    /// that would run unrecorded or unread.
    ///
    /// # Errors
    ///
    /// A failure of class `input`, code `plan_format_unsupported` or
    /// `plan_unreadable`, that names the kind of part and never its content.
    pub fn check(&self) -> Result<(), Failure> {
        if !READABLE_FORMATS.contains(&self.header.format) {
            return Err(refused(
                FailureCode::PLAN_FORMAT_UNSUPPORTED,
                format!(
                    "The plan is in format {} and this engine reads format {PLAN_FORMAT}.",
                    self.header.format
                ),
            ));
        }
        let expr = |index: ExprIndex| within("expression", index.get(), self.expressions.len());
        let optional_expr = |index: Option<ExprIndex>| index.map_or(Ok(()), expr);
        let surface = |index: SurfaceIndex| within("surface", index.get(), self.surfaces.len());
        let field_type =
            |index: FieldTypeIndex| within("field type", index.get(), self.field_types.len());
        let args = |args: &[StepArg]| args.iter().try_for_each(|arg| expr(arg.value));
        let inputs = |inputs: &[InputPlan]| {
            inputs
                .iter()
                .try_for_each(|input| field_type(input.field_type))
        };
        let steps = |steps: &[Step]| {
            steps.iter().try_for_each(|step| {
                args(&step.args)?;
                optional_expr(step.when)?;
                step.refers_to.as_ref().map_or(Ok(()), |reference| {
                    within("pack section", reference.section.get(), self.packs.len())
                })
            })
        };

        within(
            "environment",
            self.target.default_environment.get(),
            self.environments.len(),
        )?;
        for item in &self.surfaces {
            item.same_session_as.map_or(Ok(()), surface)?;
            if let Some(Stands::Beneath(owner)) = item.stands {
                surface(owner)?;
            }
            for program in &item.programs {
                if !DeclaredProgram::is_absolute_path(&program.path) {
                    return Err(refused(
                        FailureCode::PLAN_UNREADABLE,
                        "The plan declares a program by a path that is not absolute.",
                    ));
                }
                if !DeclaredProgram::is_direct_path(&program.path) {
                    return Err(refused(
                        FailureCode::PLAN_UNREADABLE,
                        "The plan declares a program by a path that steps through `.` or `..`.",
                    ));
                }
                if DeclaredProgram::is_share_path(&program.path) && program.digest.is_none() {
                    return Err(refused(
                        FailureCode::PLAN_UNREADABLE,
                        "The plan declares a program on a network share and states no digest for it.",
                    ));
                }
            }
        }
        let mut signed_in: Vec<SurfaceIndex> = Vec::new();
        for entry in &self.auth {
            for index in &entry.surfaces {
                surface(*index)?;
                if signed_in.contains(index) {
                    return Err(refused(
                        FailureCode::PLAN_UNREADABLE,
                        format!("The plan signs in to surface {} in two ways.", index.get()),
                    ));
                }
                signed_in.push(*index);
            }
            match &entry.probe {
                ProbePlan::Operation { operation, .. } => {
                    within("operation", operation.get(), self.operations.len())?;
                }
                ProbePlan::Entry(reference) => {
                    within("pack section", reference.section.get(), self.packs.len())?;
                }
            }
        }
        for environment in &self.environments {
            for address in &environment.addresses {
                surface(address.surface)?;
            }
        }
        for item in &self.field_types {
            optional_expr(item.normalizer)?;
        }
        for operation in &self.operations {
            inputs(&operation.inputs)?;
            for output in &operation.outputs {
                output.field_type.map_or(Ok(()), field_type)?;
            }
            for key in &operation.identity.key {
                within("input", key.get(), operation.inputs.len())?;
            }
            for index in &operation.manifest.surfaces {
                surface(*index)?;
            }
            for binding in &operation.bindings {
                surface(binding.surface)?;
                optional_expr(binding.precondition)?;
                for list in [
                    &binding.act,
                    &binding.confirm,
                    &binding.read_back,
                    &binding.lookup,
                ] {
                    steps(list)?;
                }
            }
        }
        for pipeline in &self.pipelines {
            inputs(&pipeline.inputs)?;
            if let Some(source) = &pipeline.source {
                args(&source.mapping)?;
                optional_expr(source.rows)?;
            }
            for step in &pipeline.steps {
                within("operation", step.operation.get(), self.operations.len())?;
                args(&step.with)?;
                optional_expr(step.for_each)?;
                optional_expr(step.when)?;
                optional_expr(step.expect)?;
            }
            pipeline.success.iter().try_for_each(|index| expr(*index))?;
        }
        for tool in &self.tools {
            match tool.runs {
                ToolRuns::Operation(index) => {
                    within("operation", index.get(), self.operations.len())?;
                }
                ToolRuns::Pipeline(index) => {
                    within("pipeline", index.get(), self.pipelines.len())?;
                }
            }
        }
        for test in &self.tests {
            within("tool", test.tool.get(), self.tools.len())?;
        }
        self.check_writes()
    }

    /// The rules that make a plan safe to run, whoever made it. The
    /// compiler refuses each of these with a place and a fix; they are held
    /// here as well, so that a plan the compiler did not make cannot run a
    /// write as a read, with nothing recorded, or with nothing read back.
    /// Called after the references are known to hold.
    fn check_writes(&self) -> Result<(), Failure> {
        let unsafe_plan = |position: usize, what: &str| {
            Err(refused(
                FailureCode::PLAN_UNREADABLE,
                format!("Operation {position} of the plan {what}."),
            ))
        };
        for (position, operation) in self.operations.iter().enumerate() {
            let writes = operation.effect >= EffectClass::Write;
            if operation.hazard != Hazard::None && operation.effect != EffectClass::Irreversible {
                return unsafe_plan(position, "has a hazard and is not irreversible");
            }
            if operation.effect == EffectClass::Write
                && operation.identity.strategy == IdentityStrategy::None
            {
                return unsafe_plan(position, "writes and cannot find its record again");
            }
            if writes && operation.commit_points == 0 {
                return unsafe_plan(position, "writes and has no commit point");
            }
            if !writes && operation.commit_points != 0 {
                return unsafe_plan(position, "reads and states commit points");
            }
            let needs_read_back = operation.effect == EffectClass::Write
                || (operation.effect == EffectClass::Irreversible
                    && operation.identity.strategy != IdentityStrategy::None);
            for binding in &operation.bindings {
                let commits = |steps: &[Step]| steps.iter().filter(|step| step.commits).count();
                if commits(&binding.confirm)
                    + commits(&binding.read_back)
                    + commits(&binding.lookup)
                    > 0
                {
                    return unsafe_plan(position, "commits in a step that does not do the work");
                }
                let saves = commits(&binding.act);
                if !writes && saves > 0 {
                    return unsafe_plan(position, "reads and has a step that commits");
                }
                if writes && u32::try_from(saves).ok() != Some(operation.commit_points) {
                    return unsafe_plan(
                        position,
                        "has a binding whose committing steps are not its commit points",
                    );
                }
                if binding.evidence == WriteEvidence::Acknowledged {
                    if operation.effect != EffectClass::Irreversible {
                        return unsafe_plan(
                            position,
                            "has a binding whose only evidence is an acknowledgement and is not irreversible",
                        );
                    }
                    // A record found by its key and read back by the
                    // binding is verified by that read, never ended on the
                    // answer alone (decision 0193).
                    if !acknowledgement_may_stand(operation.identity.strategy, &binding.read_back) {
                        return unsafe_plan(
                            position,
                            "has a binding that reads its record back by its key and ends it on an acknowledgement",
                        );
                    }
                } else if needs_read_back && binding.read_back.is_empty() {
                    return unsafe_plan(position, "writes and has a binding with no read-back");
                }
            }
        }
        for (position, pipeline) in self.pipelines.iter().enumerate() {
            for step in &pipeline.steps {
                // The reference was checked above.
                let Some(operation) = self.operation(step.operation) else {
                    continue;
                };
                if pipeline.effect < operation.effect || pipeline.hazard < operation.hazard {
                    return Err(refused(
                        FailureCode::PLAN_UNREADABLE,
                        format!(
                            "Pipeline {position} of the plan states less than its operations do."
                        ),
                    ));
                }
            }
        }
        for (position, tool) in self.tools.iter().enumerate() {
            let effect = match tool.runs {
                ToolRuns::Operation(index) => self.operation(index).map(|found| found.effect),
                ToolRuns::Pipeline(index) => self.pipeline(index).map(|found| found.effect),
            };
            if tool.annotations.read_only && effect != Some(EffectClass::Read) {
                return Err(refused(
                    FailureCode::PLAN_UNREADABLE,
                    format!("Tool {position} of the plan is described as read-only and writes."),
                ));
            }
        }
        Ok(())
    }

    /// Encodes the plan: a first line that names the format, then the plan
    /// as canonical JSON. The same plan gives the same bytes on every
    /// machine and every run, and the hash is the digest of all of them.
    ///
    /// # Errors
    ///
    /// What [`Plan::check`] refuses.
    pub fn encode(&self) -> Result<EncodedPlan, Failure> {
        self.check()?;
        let body = canonical::to_bytes(self).map_err(canonical::not_encodable)?;
        let mut bytes = Vec::with_capacity(ENVELOPE.len() + 8 + body.len());
        bytes.extend_from_slice(ENVELOPE);
        bytes.extend_from_slice(self.header.format.to_string().as_bytes());
        bytes.push(b'\n');
        bytes.extend_from_slice(&body);
        let hash = Digest::of(&bytes);
        Ok(EncodedPlan { bytes, hash })
    }

    /// Reads a plan from bytes, after checking that they are the bytes the
    /// plan was published with. The hash is checked before anything is read.
    ///
    /// # Errors
    ///
    /// Failures of class `input`: `plan_hash_mismatch` when the digest of
    /// `bytes` is not `expected`; `plan_format_unsupported` when the format
    /// is not one this engine reads; `plan_unreadable` when the bytes are not
    /// a plan that holds together.
    pub fn decode(bytes: &[u8], expected: &Digest) -> Result<Self, Failure> {
        if Digest::of(bytes) != *expected {
            return Err(refused(
                FailureCode::PLAN_HASH_MISMATCH,
                "The plan's bytes are not the bytes it was published with.",
            ));
        }
        let not_a_plan = || {
            refused(
                FailureCode::PLAN_UNREADABLE,
                "The bytes do not start as an Anything2MCP plan does.",
            )
        };
        let rest = bytes.strip_prefix(ENVELOPE).ok_or_else(not_a_plan)?;
        let line_end = rest
            .iter()
            .position(|byte| *byte == b'\n')
            .ok_or_else(not_a_plan)?;
        let (number, body) = rest.split_at(line_end);
        let format = std::str::from_utf8(number)
            .ok()
            .filter(|text| text.len() <= 5 && text.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|text| text.parse::<u16>().ok())
            .ok_or_else(not_a_plan)?;
        if !READABLE_FORMATS.contains(&format) {
            return Err(refused(
                FailureCode::PLAN_FORMAT_UNSUPPORTED,
                format!(
                    "The plan is in format {format} and this engine reads format {PLAN_FORMAT}."
                ),
            ));
        }
        let plan: Self = serde_json::from_slice(&body[1..]).map_err(|error| {
            refused(
                FailureCode::PLAN_UNREADABLE,
                format!(
                    "The plan's body is not a plan of format {format}, at character {} of {}.",
                    error.column(),
                    body.len() - 1
                ),
            )
        })?;
        if plan.header.format != format {
            return Err(refused(
                FailureCode::PLAN_UNREADABLE,
                "The plan's first line and its header name different formats.",
            ));
        }
        plan.check()?;
        Ok(plan)
    }

    /// The plan as stable text, for `plan.show`: its hash, then the same
    /// content as the encoding, in the same order, laid out for a person to
    /// read and to compare with another plan line by line. Lines end with a
    /// line feed on every machine.
    ///
    /// # Errors
    ///
    /// What [`Plan::check`] refuses.
    pub fn show(&self) -> Result<String, Failure> {
        let encoded = self.encode()?;
        let body = canonical::to_readable(self).map_err(canonical::not_encodable)?;
        Ok(format!(
            "plan {} {}\nformat {}\nhash {}\n{body}\n",
            self.header.target, self.header.version, self.header.format, encoded.hash
        ))
    }
}

/// Whether a binding whose evidence is `acknowledged` may end its record on
/// the system's answer alone: only where the record cannot be read back by
/// a key. That is a record of identity `none`, or one whose binding has no
/// read-back step, such as a queue take, whose message is gone once it is
/// acknowledged. A record found by its key that the binding reads back is
/// verified by that read (spec section 208; decision 0193). The compiler
/// and the plan's check ask this one question.
#[must_use]
pub fn acknowledgement_may_stand(strategy: IdentityStrategy, read_back: &[Step]) -> bool {
    strategy == IdentityStrategy::None || read_back.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Date, Decimal};

    fn payload(text: &str) -> PackPayload {
        PackPayload::encode(&Values::new().with("template", text)).unwrap()
    }

    fn step(kind: &str, arg: u32) -> Step {
        Step {
            kind: StepKind::new(kind).unwrap(),
            refers_to: Some(PackRef {
                section: SectionIndex::new(0),
                entry: "order".to_owned(),
            }),
            args: vec![StepArg {
                name: "order_id".to_owned(),
                value: ExprIndex::new(arg),
            }],
            timeout_ms: 10_000,
            fails_as: FailsAs {
                class: FailureClass::Drift,
                code: FailureCode::new("response_changed").unwrap(),
            },
            commits: false,
            lookup: false,
            idempotent: false,
            when: None,
            payload: payload("/orders/{order_id}"),
        }
    }

    fn input(name: &str, ty: ValueType, field_type: u32) -> InputPlan {
        InputPlan {
            name: name.to_owned(),
            field_type: FieldTypeIndex::new(field_type),
            ty,
            required: true,
            default: None,
            description: format!("The {name} of the order."),
        }
    }

    /// A small plan for the imaginary `example` target that uses every part.
    fn sample() -> Plan {
        let save = Step {
            commits: true,
            idempotent: true,
            when: Some(ExprIndex::new(2)),
            ..step("request", 0)
        };
        let find = Step {
            lookup: true,
            ..step("request", 0)
        };
        let update = OperationPlan {
            name: OperationName::new("example_order_update").unwrap(),
            effect: EffectClass::Write,
            hazard: Hazard::None,
            inputs: vec![
                input("order_id", ValueType::Text, 0),
                InputPlan {
                    required: false,
                    default: Some(Value::Decimal(Decimal::parse("0.00").unwrap())),
                    ..input("total", ValueType::Decimal, 1)
                },
            ],
            outputs: vec![
                OutputPlan {
                    name: "total".to_owned(),
                    ty: ValueType::Decimal,
                    field_type: Some(FieldTypeIndex::new(1)),
                },
                OutputPlan {
                    name: "lines".to_owned(),
                    ty: ValueType::list_of(ValueType::Record(vec![crate::FieldOf::required(
                        "code",
                        ValueType::Choice,
                    )])),
                    field_type: None,
                },
            ],
            identity: IdentityPlan {
                strategy: IdentityStrategy::Natural,
                key: vec![InputIndex::new(0)],
            },
            commit_points: 1,
            bindings: vec![BindingPlan {
                mode: Mode::new("http").unwrap(),
                verb: Verb::new("update").unwrap(),
                status: BindingStatus::Verified,
                surface: SurfaceIndex::new(0),
                act: vec![step("token", 0), save],
                confirm: vec![step("validate", 0)],
                read_back: vec![step("request", 0), step("extract", 1)],
                lookup: vec![find],
                settle_ms: 2_000,
                evidence: WriteEvidence::ReadBack,
                precondition: Some(ExprIndex::new(2)),
            }],
            manifest: PermissionManifest {
                surfaces: vec![SurfaceIndex::new(0)],
                uploads: vec![],
                downloads: vec!["artifact_out".to_owned()],
                destinations: vec!["https://files.example.test".to_owned()],
            },
            examples: vec![Values::new().with("order_id", "SO-1001")],
            decision_points: vec![DecisionPoint {
                name: "which_order".to_owned(),
                question: "Two orders match. Which one?".to_owned(),
            }],
        };
        Plan {
            header: PlanHeader {
                format: PLAN_FORMAT,
                minimum_engine: Version::new(0, 1, 0),
                compiler: Version::new(0, 1, 0),
                target: TargetName::new("example").unwrap(),
                version: Version::new(1, 4, 0),
                sources: Digest::of(b"operations/order_update.yaml"),
            },
            target: TargetPlan {
                title: "Example order entry".to_owned(),
                modules: vec!["order".to_owned()],
                default_environment: EnvironmentIndex::new(0),
                locale: Some("en-US".to_owned()),
                timezone: Some("+02:00".to_owned()),
                capture: Some(CaptureLevel::Standard),
                exclusive_session: true,
                drift_risk: Some(DriftRisk::Low),
                authorization: Some("wrap-2026-11".to_owned()),
            },
            surfaces: vec![
                SurfacePlan {
                    name: SurfaceName::new("api").unwrap(),
                    kind: SurfaceKind::new("http").unwrap(),
                    address: "https://api.example.test".to_owned(),
                    same_session_as: None,
                    stands: Some(Stands::Alone),
                    rate: Some(RatePlan {
                        requests: 10,
                        per_ms: 1_000,
                    }),
                    programs: vec![
                        DeclaredProgram {
                            path: "/opt/example/bin/example-tool".to_owned(),
                            digest: Some(Digest::of(b"example-tool")),
                        },
                        DeclaredProgram {
                            path: "C:\\example\\example-tool.exe".to_owned(),
                            digest: None,
                        },
                    ],
                    payload: PackPayload::none(),
                },
                SurfacePlan {
                    name: SurfaceName::new("site").unwrap(),
                    kind: SurfaceKind::new("web").unwrap(),
                    address: "https://www.example.test".to_owned(),
                    same_session_as: Some(SurfaceIndex::new(0)),
                    stands: Some(Stands::Beneath(SurfaceIndex::new(0))),
                    rate: None,
                    programs: vec![],
                    payload: PackPayload::none(),
                },
            ],
            environments: vec![EnvironmentPlan {
                name: Environment::new("test").unwrap(),
                kind: EnvironmentKind::Test,
                addresses: vec![AddressOverride {
                    surface: SurfaceIndex::new(0),
                    address: "https://api.test.example.test".to_owned(),
                }],
                test_records: vec![
                    RecordKey::new(vec![Value::from("SO-1001")]).unwrap(),
                    RecordKey::new(vec![Value::from("SO-1001"), Value::Int(7)]).unwrap(),
                ],
                approve_writes: Some(ApproveWrites::NamedApprover),
            }],
            auth: vec![AuthPlan {
                surfaces: vec![SurfaceIndex::new(0)],
                mode: SignInMode::Custom,
                provider: AuthProviderName::new("api_key").unwrap(),
                probe: ProbePlan::Operation {
                    operation: OperationIndex::new(0),
                    inputs: Values::new().with("order_id", "SO-1001"),
                },
                expiry_signals: vec![payload("status 401")],
                idle_ms: Some(1_800_000),
                absolute_ms: None,
                sign_out: Some("sign_out".to_owned()),
                settings: vec![AuthSetting {
                    name: "header".to_owned(),
                    value: "X-Example-Key".to_owned(),
                }],
                credentials: vec![CredentialUse {
                    role: "key".to_owned(),
                    reference: CredentialRef::new("vault://example/service/key").unwrap(),
                }],
                attempt_limit: 1,
                movable: true,
            }],
            field_types: vec![
                FieldTypePlan {
                    name: "identifier".to_owned(),
                    base: FieldBase::Text,
                    ty: ValueType::Text,
                    places: None,
                    rounding: None,
                    step: None,
                    unit: None,
                    display_unit: None,
                    currency: None,
                    zone: None,
                    codes: vec![],
                    sensitivity: Sensitivity::Internal,
                    untrusted: false,
                    format: FieldFormat::Text {
                        case_sensitive: true,
                        whitespace: WhitespaceRule::Trim,
                    },
                    normalizer: None,
                },
                FieldTypePlan {
                    name: "money".to_owned(),
                    base: FieldBase::Money,
                    ty: ValueType::Decimal,
                    places: Some(2),
                    rounding: Some(Rounding::HalfEven),
                    step: Some(Decimal::parse("0.05").unwrap()),
                    unit: None,
                    display_unit: None,
                    currency: Some("EUR".to_owned()),
                    zone: Some(ZoneRule::Named("Europe/Example".to_owned())),
                    codes: vec![CodeLabel {
                        code: "A".to_owned(),
                        label: "Active".to_owned(),
                    }],
                    sensitivity: Sensitivity::Restricted,
                    untrusted: true,
                    format: FieldFormat::Number {
                        decimal_mark: DecimalMark::Comma,
                    },
                    normalizer: Some(ExprIndex::new(1)),
                },
            ],
            expressions: vec![
                PlanExpr {
                    source: "input.order_id".to_owned(),
                    result: ValueType::Text,
                    library: 1,
                },
                PlanExpr {
                    source: "round_to(parse_decimal(raw), 2, \"half_even\")".to_owned(),
                    result: ValueType::Decimal,
                    library: 1,
                },
                PlanExpr {
                    source: "input.total != null".to_owned(),
                    result: ValueType::Bool,
                    library: 1,
                },
            ],
            operations: vec![update],
            pipelines: vec![PipelinePlan {
                name: OperationName::new("example_orders_import").unwrap(),
                effect: EffectClass::Write,
                hazard: Hazard::None,
                inputs: vec![input("batch", ValueType::Text, 0)],
                source: Some(SourcePlan {
                    template: "order_sheet 3".to_owned(),
                    reader: "csv".to_owned(),
                    mapping: vec![StepArg {
                        name: "order_id".to_owned(),
                        value: ExprIndex::new(0),
                    }],
                    rows: Some(ExprIndex::new(2)),
                    payload: payload("header: Order"),
                }),
                limits: PipelineLimits {
                    max_records: Some(500),
                    max_duration_ms: Some(600_000),
                },
                steps: vec![PipelineStep {
                    id: "update".to_owned(),
                    operation: OperationIndex::new(0),
                    with: vec![StepArg {
                        name: "order_id".to_owned(),
                        value: ExprIndex::new(0),
                    }],
                    for_each: Some(ExprIndex::new(0)),
                    checkpoint: Some(Checkpoint::PerRecord),
                    when: Some(ExprIndex::new(2)),
                    expect: Some(ExprIndex::new(2)),
                }],
                success: vec![ExprIndex::new(2)],
                headroom_ms: 60_000,
            }],
            tools: vec![
                ToolPlan {
                    name: OperationName::new("example_order_update").unwrap(),
                    runs: ToolRuns::Operation(OperationIndex::new(0)),
                    module: Some("order".to_owned()),
                    title: "Update an order".to_owned(),
                    description: "Sets the total of one order. Naïve text: \"quoted\", ünïcode, and a\nline end.".to_owned(),
                    input_schema: payload("object"),
                    output_schema: payload("object"),
                    annotations: ToolAnnotations {
                        read_only: false,
                        destructive: false,
                        idempotent: true,
                        open_world: false,
                    },
                },
                ToolPlan {
                    name: OperationName::new("example_orders_import").unwrap(),
                    runs: ToolRuns::Pipeline(PipelineIndex::new(0)),
                    module: None,
                    title: "Import orders".to_owned(),
                    description: "Updates every order in a sheet.".to_owned(),
                    input_schema: payload("object"),
                    output_schema: payload("object"),
                    annotations: ToolAnnotations::default(),
                },
            ],
            packs: vec![PackSection {
                pack: "example_http".to_owned(),
                kind: FileKindName::new("endpoint").unwrap(),
                surface_kind: SurfaceKind::new("http").unwrap(),
                format: 1,
                payload: payload("GET /orders/{order_id}"),
            }],
            tests: vec![TestPlan {
                name: "update_sets_the_total".to_owned(),
                tool: ToolIndex::new(0),
                kind: TestKind::Offline,
                fixture: Some("fixtures/order_update".to_owned()),
                inputs: Values::new()
                    .with("total", Decimal::parse("12.50").unwrap())
                    .with("order_id", "SO-1001")
                    .with("due", Date::new(2026, 11, 17).unwrap()),
                expect: payload("verified"),
                restore: false,
            }],
        }
    }

    /// The hash of the sample plan. The encoding must give these bytes on
    /// Windows, macOS, and Linux, in every run. If the encoding or the
    /// sample changes on purpose, this changes with it, and the plan format
    /// number says whether stored plans still load.
    const SAMPLE_HASH: &str = "a0c8f809caff95d328419cdc02052e9ae6d4d81d7f27c7c1463c354f20e45d2a";

    #[test]
    fn a_plan_encodes_to_the_same_bytes_every_time_and_on_every_machine() {
        let first = sample().encode().unwrap();
        let second = sample().encode().unwrap();
        assert_eq!(first.bytes, second.bytes);
        assert_eq!(first.hash, Digest::of(&first.bytes));
        assert_eq!(first.hash.to_string(), SAMPLE_HASH);
        assert!(first.bytes.starts_with(b"anything2mcp-plan 1\n{"));
        assert!(!first.bytes.contains(&b'\r'));
    }

    #[test]
    fn a_plan_decodes_to_an_equal_plan_and_encodes_back_to_the_same_bytes() {
        let plan = sample();
        let encoded = plan.encode().unwrap();
        let decoded = Plan::decode(&encoded.bytes, &encoded.hash).unwrap();
        assert_eq!(decoded, plan);
        assert_eq!(decoded.encode().unwrap().bytes, encoded.bytes);
    }

    #[test]
    fn the_order_values_were_added_in_does_not_reach_the_bytes() {
        let mut turned = sample();
        turned.tests[0].inputs = Values::new()
            .with("due", Date::new(2026, 11, 17).unwrap())
            .with("order_id", "SO-1001")
            .with("total", Decimal::parse("12.5").unwrap());
        assert_eq!(
            turned.encode().unwrap().bytes,
            sample().encode().unwrap().bytes
        );
    }

    #[test]
    fn a_plan_whose_bytes_changed_by_one_bit_fails_its_hash_check() {
        // Every bit of the first line and the start of the body, and one bit
        // of every byte after that, a different one from byte to byte.
        let encoded = sample().encode().unwrap();
        let mut bytes = encoded.bytes.clone();
        for position in 0..bytes.len() {
            let bits = if position < 64 {
                0..8
            } else {
                position % 8..position % 8 + 1
            };
            for bit in bits {
                bytes[position] ^= 1 << bit;
                let failure = Plan::decode(&bytes, &encoded.hash).unwrap_err();
                assert_eq!(failure.class(), FailureClass::Input);
                assert_eq!(
                    failure.code(),
                    &FailureCode::PLAN_HASH_MISMATCH,
                    "byte {position}, bit {bit}"
                );
                bytes[position] ^= 1 << bit;
            }
        }
        assert_eq!(bytes, encoded.bytes);
    }

    #[test]
    fn bytes_cut_short_or_added_to_fail_the_hash_check() {
        let encoded = sample().encode().unwrap();
        let cut = &encoded.bytes[..encoded.bytes.len() - 1];
        let mut longer = encoded.bytes.clone();
        longer.push(b' ');
        for bytes in [cut, longer.as_slice(), b"".as_slice()] {
            let failure = Plan::decode(bytes, &encoded.hash).unwrap_err();
            assert_eq!(failure.code(), &FailureCode::PLAN_HASH_MISMATCH);
        }
        // The right bytes under another plan's hash are refused as well.
        let other = Digest::of(b"another plan");
        let failure = Plan::decode(&encoded.bytes, &other).unwrap_err();
        assert_eq!(failure.code(), &FailureCode::PLAN_HASH_MISMATCH);
    }

    /// Decodes bytes under their own digest, so only what follows the hash
    /// check can refuse them.
    fn decode_own(bytes: &[u8]) -> Result<Plan, Failure> {
        Plan::decode(bytes, &Digest::of(bytes))
    }

    #[test]
    fn a_format_this_engine_does_not_read_is_refused_by_name() {
        let mut newer = sample();
        newer.header.format = PLAN_FORMAT + 1;
        let failure = newer.encode().unwrap_err();
        assert_eq!(failure.code(), &FailureCode::PLAN_FORMAT_UNSUPPORTED);

        let failure = decode_own(b"anything2mcp-plan 2\n{}").unwrap_err();
        assert_eq!(failure.class(), FailureClass::Input);
        assert_eq!(failure.code(), &FailureCode::PLAN_FORMAT_UNSUPPORTED);
        assert!(
            failure.next_step().contains("migrate"),
            "{}",
            failure.next_step()
        );
        let failure = decode_own(b"anything2mcp-plan 0\n{}").unwrap_err();
        assert_eq!(failure.code(), &FailureCode::PLAN_FORMAT_UNSUPPORTED);
    }

    #[test]
    fn bytes_with_the_right_hash_that_are_not_a_plan_are_refused() {
        let good = sample().encode().unwrap().bytes;
        let body =
            String::from_utf8(good[good.iter().position(|b| *b == b'\n').unwrap() + 1..].to_vec())
                .unwrap();
        let with_line = |line: &str, body: &str| [line.as_bytes(), body.as_bytes()].concat();

        let unknown_field = body.replacen("{\"auth\"", "{\"added\":1,\"auth\"", 1);
        assert_ne!(unknown_field, body);
        let with_float = body.replacen("\"commit_points\":1", "\"commit_points\":1.0", 1);
        assert_ne!(with_float, body);
        let other_format_inside = body.replacen(
            "\"format\":1,\"minimum_engine\"",
            "\"format\":7,\"minimum_engine\"",
            1,
        );
        assert_ne!(other_format_inside, body);

        let cases: Vec<Vec<u8>> = vec![
            b"".to_vec(),
            b"{}".to_vec(),
            b"anything2mcp-plan 1".to_vec(),
            b"anything2mcp-plan one\n{}".to_vec(),
            b"anything2mcp-plan 100000\n{}".to_vec(),
            b"anything2mcp-plan 1\n".to_vec(),
            b"anything2mcp-plan 1\n{}".to_vec(),
            b"anything2mcp-plan 1\r\n{}".to_vec(),
            with_line("anything2mcp-plan 1\n", &body[..body.len() - 1]),
            with_line("anything2mcp-plan 1\n", &unknown_field),
            with_line("anything2mcp-plan 1\n", &with_float),
            with_line("anything2mcp-plan 1\n", &other_format_inside),
        ];
        for bytes in cases {
            let failure = decode_own(&bytes).unwrap_err();
            assert_eq!(failure.class(), FailureClass::Input);
            assert_eq!(
                failure.code(),
                &FailureCode::PLAN_UNREADABLE,
                "{}",
                String::from_utf8_lossy(&bytes[..bytes.len().min(60)])
            );
            // A refusal gives a place, never the plan's content.
            assert!(!failure.message().contains("example"), "{failure}");
        }
    }

    #[test]
    fn a_plan_with_a_loose_reference_is_refused_on_the_way_out_and_on_the_way_in() {
        type Loosen = fn(&mut Plan);
        let loosen: [(&str, Loosen); 15] = [
            ("environment", |plan| {
                plan.target.default_environment = EnvironmentIndex::new(1);
            }),
            ("surface", |plan| {
                plan.surfaces[1].stands = Some(Stands::Beneath(SurfaceIndex::new(2)));
            }),
            ("surface", |plan| {
                plan.auth[0].surfaces = vec![SurfaceIndex::new(2)];
            }),
            ("two ways", |plan| {
                plan.auth.push(plan.auth[0].clone());
            }),
            ("operation", |plan| {
                plan.auth[0].probe = ProbePlan::Operation {
                    operation: OperationIndex::new(1),
                    inputs: Values::new(),
                };
            }),
            ("pack section", |plan| {
                plan.auth[0].probe = ProbePlan::Entry(PackRef {
                    section: SectionIndex::new(1),
                    entry: "order".to_owned(),
                });
            }),
            ("expression", |plan| {
                plan.operations[0].bindings[0].read_back[0].args[0].value = ExprIndex::new(99);
            }),
            ("expression", |plan| {
                plan.operations[0].bindings[0].lookup[0].when = Some(ExprIndex::new(3));
            }),
            ("surface", |plan| {
                plan.operations[0].bindings[0].surface = SurfaceIndex::new(2);
            }),
            ("field type", |plan| {
                plan.operations[0].inputs[1].field_type = FieldTypeIndex::new(2);
            }),
            ("input", |plan| {
                plan.operations[0].identity.key = vec![InputIndex::new(2)];
            }),
            ("pack section", |plan| {
                plan.operations[0].bindings[0].act[0].refers_to = Some(PackRef {
                    section: SectionIndex::new(1),
                    entry: "order".to_owned(),
                });
            }),
            ("operation", |plan| {
                plan.pipelines[0].steps[0].operation = OperationIndex::new(1);
            }),
            ("pipeline", |plan| {
                plan.tools[1].runs = ToolRuns::Pipeline(PipelineIndex::new(1));
            }),
            ("tool", |plan| {
                plan.tests[0].tool = ToolIndex::new(2);
            }),
        ];
        let good = String::from_utf8(sample().encode().unwrap().bytes).unwrap();
        for (what, change) in loosen {
            let mut plan = sample();
            change(&mut plan);
            let failure = plan.encode().unwrap_err();
            assert_eq!(failure.code(), &FailureCode::PLAN_UNREADABLE, "{what}");
            assert!(failure.message().contains(what), "{failure}");
        }
        // The same on the way in: bytes that say what a loose plan would.
        let loose = good.replacen("\"tool\":0", "\"tool\":2", 1);
        assert_ne!(loose, good);
        let failure = decode_own(loose.as_bytes()).unwrap_err();
        assert_eq!(failure.code(), &FailureCode::PLAN_UNREADABLE);
        assert!(failure.message().contains("tool 2"), "{failure}");
    }

    /// The sample's write, turned into a read with one step that reads.
    fn as_read(plan: &mut Plan) {
        let operation = &mut plan.operations[0];
        operation.effect = EffectClass::Read;
        operation.commit_points = 0;
        operation.bindings[0].act = vec![step("request", 0)];
        plan.pipelines[0].effect = EffectClass::Read;
    }

    #[test]
    fn a_plan_that_breaks_a_rule_of_a_write_is_refused_on_the_way_out_and_on_the_way_in() {
        type Break = fn(&mut Plan);
        let breaks: [(&str, Break); 14] = [
            // A write that says it reads: nothing would be recorded first.
            ("reads and states commit points", |plan| {
                plan.operations[0].effect = EffectClass::Read;
            }),
            ("reads and has a step that commits", |plan| {
                plan.operations[0].effect = EffectClass::Read;
                plan.operations[0].commit_points = 0;
            }),
            // A write with nothing to record.
            ("no commit point", |plan| {
                plan.operations[0].commit_points = 0;
                plan.operations[0].bindings[0].act[1].commits = false;
            }),
            ("not its commit points", |plan| {
                plan.operations[0].bindings[0].act[1].commits = false;
            }),
            ("not its commit points", |plan| {
                plan.operations[0].bindings[0].act[0].commits = true;
            }),
            ("not its commit points", |plan| {
                plan.operations[0].commit_points = 2;
            }),
            ("does not do the work", |plan| {
                plan.operations[0].bindings[0].confirm[0].commits = true;
            }),
            ("does not do the work", |plan| {
                plan.operations[0].bindings[0].read_back[0].commits = true;
            }),
            ("does not do the work", |plan| {
                as_read(plan);
                plan.operations[0].bindings[0].lookup[0].commits = true;
            }),
            // A write nobody reads back, and one nobody could find again.
            ("no read-back", |plan| {
                plan.operations[0].bindings[0].read_back.clear();
            }),
            ("cannot find its record again", |plan| {
                plan.operations[0].identity = IdentityPlan {
                    strategy: IdentityStrategy::None,
                    key: Vec::new(),
                };
            }),
            ("has a hazard", |plan| {
                plan.operations[0].hazard = Hazard::SendsOutside;
            }),
            // An acknowledgement stands as evidence only where nothing can
            // be undone, and so nothing can be read back.
            ("only evidence is an acknowledgement", |plan| {
                plan.operations[0].bindings[0].evidence = WriteEvidence::Acknowledged;
            }),
            ("states less", |plan| {
                plan.pipelines[0].effect = EffectClass::Read;
            }),
        ];
        for (what, change) in breaks {
            let mut plan = sample();
            change(&mut plan);
            let failure = plan.encode().unwrap_err();
            assert_eq!(failure.class(), FailureClass::Input, "{what}");
            assert_eq!(failure.code(), &FailureCode::PLAN_UNREADABLE, "{what}");
            assert!(failure.message().contains(what), "{what}: {failure}");
            // The refusal names a place, never the plan's content.
            assert!(!failure.message().contains("example"), "{failure}");
        }

        // The same on the way in, for bytes the compiler did not make: the
        // sample's write, stored as a read under its own hash.
        let good = String::from_utf8(sample().encode().unwrap().bytes).unwrap();
        let as_a_read = good.replacen("\"effect\":\"write\"", "\"effect\":\"read\"", 1);
        assert_ne!(as_a_read, good);
        let failure = decode_own(as_a_read.as_bytes()).unwrap_err();
        assert_eq!(failure.code(), &FailureCode::PLAN_UNREADABLE);
        let unread = good.replacen("\"commits\":true", "\"commits\":false", 1);
        assert_ne!(unread, good);
        let failure = decode_own(unread.as_bytes()).unwrap_err();
        assert_eq!(failure.code(), &FailureCode::PLAN_UNREADABLE);
        assert!(failure.message().contains("commit point"), "{failure}");

        // An irreversible operation may take the acknowledgement alone, and
        // then needs no read-back.
        let mut plan = sample();
        plan.operations[0].effect = EffectClass::Irreversible;
        plan.pipelines[0].effect = EffectClass::Irreversible;
        plan.operations[0].bindings[0].evidence = WriteEvidence::Acknowledged;
        plan.operations[0].bindings[0].read_back.clear();
        plan.encode().unwrap();
    }

    /// Decision 0193, finding F-1: a record found by its key that its
    /// binding reads back is never ended on the acknowledgement alone,
    /// irreversible or not.
    #[test]
    fn a_keyed_record_its_binding_reads_back_is_not_ended_on_an_acknowledgement() {
        let mut plan = sample();
        plan.operations[0].effect = EffectClass::Irreversible;
        plan.pipelines[0].effect = EffectClass::Irreversible;
        plan.operations[0].bindings[0].evidence = WriteEvidence::Acknowledged;
        assert!(!plan.operations[0].bindings[0].read_back.is_empty());
        let failure = plan.encode().unwrap_err();
        assert_eq!(failure.code(), &FailureCode::PLAN_UNREADABLE);
        assert!(
            failure
                .message()
                .contains("reads its record back by its key"),
            "{failure}"
        );
        // With no key there is nothing to read it back by.
        let mut keyless = plan.clone();
        keyless.operations[0].identity = IdentityPlan {
            strategy: IdentityStrategy::None,
            key: Vec::new(),
        };
        keyless.encode().unwrap();
        // With no read-back step, the answer is the only evidence there is.
        plan.operations[0].bindings[0].read_back.clear();
        plan.encode().unwrap();
        assert!(super::acknowledgement_may_stand(
            IdentityStrategy::None,
            &sample().operations[0].bindings[0].read_back
        ));
        assert!(!super::acknowledgement_may_stand(
            IdentityStrategy::Natural,
            &sample().operations[0].bindings[0].read_back
        ));
    }

    #[test]
    fn the_plans_a_write_rule_allows_still_encode() {
        // A read commits nowhere and states no commit point.
        let mut read = sample();
        as_read(&mut read);
        read.tools[0].annotations.read_only = true;
        read.tools[1].annotations.read_only = true;
        read.encode().unwrap();
        // A tool described as read-only never runs a write.
        let mut described = sample();
        described.tools[0].annotations.read_only = true;
        let failure = described.encode().unwrap_err();
        assert!(failure.message().contains("read-only"), "{failure}");
        let mut described = sample();
        described.tools[1].annotations.read_only = true;
        assert!(described.encode().is_err());

        // An irreversible operation with no identity has no read-back to
        // give; with an identity it needs one.
        let mut sent = sample();
        sent.operations[0].effect = EffectClass::Irreversible;
        sent.operations[0].hazard = Hazard::SendsOutside;
        sent.pipelines[0].effect = EffectClass::Irreversible;
        sent.pipelines[0].hazard = Hazard::SendsOutside;
        sent.encode().unwrap();
        sent.operations[0].bindings[0].read_back.clear();
        assert!(sent.encode().is_err());
        sent.operations[0].identity = IdentityPlan {
            strategy: IdentityStrategy::None,
            key: Vec::new(),
        };
        sent.encode().unwrap();

        // Two saves are two commit points, in every binding.
        let mut twice = sample();
        twice.operations[0].commit_points = 2;
        twice.operations[0].bindings[0].act[0].commits = true;
        twice.encode().unwrap();
        let mut second = twice.operations[0].bindings[0].clone();
        second.act.pop();
        twice.operations[0].bindings.push(second);
        assert!(twice.encode().is_err());
    }

    #[test]
    fn a_declared_program_is_named_by_an_absolute_path_on_every_machine() {
        for path in [
            "/opt/example/bin/example-tool",
            "C:\\example\\example-tool.exe",
            "c:/example/example-tool.exe",
            "\\\\server\\share\\example-tool.exe",
        ] {
            assert!(DeclaredProgram::is_absolute_path(path), "{path}");
        }
        for path in [
            "",
            "example-tool",
            "bin/example-tool",
            "./example-tool",
            "..\\example-tool.exe",
            "C:example-tool.exe",
            "1:\\example-tool.exe",
            "\\example-tool.exe",
            "\\\\",
            "\\\\\\server",
            "/opt/example\0/tool",
        ] {
            assert!(!DeclaredProgram::is_absolute_path(path), "{path:?}");
        }
        // The plan carries them in its bytes, and a plan that names a
        // program any other way neither encodes nor decodes.
        let good = String::from_utf8(sample().encode().unwrap().bytes).unwrap();
        assert!(good.contains("\"path\":\"/opt/example/bin/example-tool\""));
        let mut plan = sample();
        plan.surfaces[0].programs[0].path = "example-tool".to_owned();
        let failure = plan.encode().unwrap_err();
        assert_eq!(failure.code(), &FailureCode::PLAN_UNREADABLE);
        assert!(!failure.message().contains("example"), "{failure}");
        let loose = good.replacen("\"/opt/example/bin/", "\"opt/example/bin/", 1);
        assert_ne!(loose, good);
        let failure = decode_own(loose.as_bytes()).unwrap_err();
        assert_eq!(failure.code(), &FailureCode::PLAN_UNREADABLE);

        // A path that steps back, and a program on a share with no digest,
        // are refused the same way; a share with a digest is declared.
        for path in [
            "/opt/example/../../tmp/tool",
            "/opt/./example/tool",
            "C:\\example\\..\\tool.exe",
            "C:/example/../tool.exe",
            "\\\\server\\share\\..\\tool.exe",
        ] {
            assert!(DeclaredProgram::is_absolute_path(path), "{path}");
            assert!(!DeclaredProgram::is_direct_path(path), "{path}");
            let mut plan = sample();
            plan.surfaces[0].programs[0].path = path.to_owned();
            let failure = plan.encode().unwrap_err();
            assert_eq!(failure.code(), &FailureCode::PLAN_UNREADABLE, "{path}");
            assert!(failure.message().contains("steps through"), "{failure}");
        }
        assert!(DeclaredProgram::is_direct_path(
            "/opt/example/tool.v2/..tool"
        ));
        let share = "\\\\server\\share\\example-tool.exe";
        assert!(DeclaredProgram::is_share_path(share));
        assert!(!DeclaredProgram::is_share_path(
            "/opt/example/bin/example-tool"
        ));
        let mut plan = sample();
        plan.surfaces[0].programs[0].path = share.to_owned();
        plan.encode().unwrap();
        plan.surfaces[0].programs[0].digest = None;
        let failure = plan.encode().unwrap_err();
        assert_eq!(failure.code(), &FailureCode::PLAN_UNREADABLE);
        assert!(failure.message().contains("network share"), "{failure}");
    }

    #[test]
    fn no_part_of_a_plan_is_written_as_a_float() {
        fn walk(value: &serde_json::Value, path: &str) {
            match value {
                serde_json::Value::Number(number) => {
                    assert!(number.is_i64() || number.is_u64(), "a float at {path}");
                }
                serde_json::Value::Array(items) => {
                    for (index, item) in items.iter().enumerate() {
                        walk(item, &format!("{path}[{index}]"));
                    }
                }
                serde_json::Value::Object(map) => {
                    for (key, item) in map {
                        walk(item, &format!("{path}.{key}"));
                    }
                }
                _ => {}
            }
        }
        let encoded = sample().encode().unwrap();
        let body = &encoded.bytes[encoded.bytes.iter().position(|b| *b == b'\n').unwrap() + 1..];
        walk(&serde_json::from_slice(body).unwrap(), "plan");
        // A decimal travels as text.
        assert!(String::from_utf8_lossy(body).contains(r#"{"decimal":"12.5"}"#));
    }

    #[test]
    fn a_sign_in_is_found_for_a_surface_and_for_one_that_shares_it() {
        let mut plan = sample();
        let entry = plan.auth_of(SurfaceIndex::new(0)).unwrap();
        assert_eq!(entry.provider.as_str(), "api_key");
        // The second surface shares the first one's sign-in.
        assert_eq!(plan.auth_of(SurfaceIndex::new(1)), Some(entry));
        assert!(plan.auth_of(SurfaceIndex::new(2)).is_none());
        // A surface nobody signs in to has none, however the surfaces point
        // at each other.
        plan.auth.clear();
        plan.surfaces[0].same_session_as = Some(SurfaceIndex::new(1));
        assert!(plan.auth_of(SurfaceIndex::new(0)).is_none());

        let plan = sample();
        assert_eq!(plan.default_environment().unwrap().name.as_str(), "test");
        let name = Environment::new("test").unwrap();
        assert_eq!(
            plan.environment_named(&name).unwrap().kind,
            EnvironmentKind::Test
        );
        assert!(
            plan.environment_named(&Environment::new("production").unwrap())
                .is_none()
        );
    }

    #[test]
    fn a_credential_in_a_plan_is_a_reference_and_bytes_that_hold_anything_else_are_refused() {
        let good = String::from_utf8(sample().encode().unwrap().bytes).unwrap();
        assert!(good.contains("\"reference\":\"vault://example/service/key\""));
        let with_value = good.replacen("vault://example/service/key", "correct horse battery", 1);
        assert_ne!(with_value, good);
        let failure = decode_own(with_value.as_bytes()).unwrap_err();
        assert_eq!(failure.code(), &FailureCode::PLAN_UNREADABLE);
        assert!(!failure.message().contains("horse"), "{failure}");
    }

    #[test]
    fn a_zone_west_of_utc_travels_as_a_whole_number_below_zero() {
        let mut plan = sample();
        plan.field_types[0].format = FieldFormat::Calendar {
            pattern: "YYYY-MM-DD HH:mm".to_owned(),
            utc_offset_minutes: Some(-330),
        };
        let encoded = plan.encode().unwrap();
        assert!(String::from_utf8_lossy(&encoded.bytes).contains("\"utc_offset_minutes\":-330"));
        assert_eq!(Plan::decode(&encoded.bytes, &encoded.hash).unwrap(), plan);
    }

    #[test]
    fn the_parts_of_a_plan_are_found_by_index_and_by_name() {
        let plan = sample();
        assert_eq!(
            plan.expression(ExprIndex::new(0)).unwrap().source,
            "input.order_id"
        );
        assert!(plan.expression(ExprIndex::new(3)).is_none());
        assert_eq!(
            plan.surface(SurfaceIndex::new(1)).unwrap().name.as_str(),
            "site"
        );
        assert_eq!(
            plan.field_type(FieldTypeIndex::new(1)).unwrap().name,
            "money"
        );
        assert_eq!(
            plan.operation(OperationIndex::new(0))
                .unwrap()
                .commit_points,
            1
        );
        assert_eq!(plan.pipeline(PipelineIndex::new(0)).unwrap().steps.len(), 1);
        assert_eq!(
            plan.section(SectionIndex::new(0)).unwrap().pack,
            "example_http"
        );
        let name = OperationName::new("example_orders_import").unwrap();
        assert_eq!(
            plan.tool(&name).unwrap().runs,
            ToolRuns::Pipeline(PipelineIndex::new(0))
        );
        assert!(
            plan.tool(&OperationName::new("example_missing").unwrap())
                .is_none()
        );
        let endpoint = FileKindName::new("endpoint").unwrap();
        assert!(
            plan.section_of(&endpoint, &SurfaceKind::new("http").unwrap())
                .is_some()
        );
        assert!(
            plan.section_of(&endpoint, &SurfaceKind::new("web").unwrap())
                .is_none()
        );
    }

    #[test]
    fn a_plan_is_shown_as_stable_text() {
        let shown = sample().show().unwrap();
        assert_eq!(shown, sample().show().unwrap());
        assert!(
            shown.starts_with(&format!(
                "plan example 1.4.0\nformat 1\nhash {SAMPLE_HASH}\n{{\n"
            )),
            "{shown}"
        );
        assert!(
            shown.contains("\n      \"name\": \"example_order_update\""),
            "{shown}"
        );
        assert!(shown.ends_with("}\n"));
        assert!(!shown.contains('\r'));
    }
}
