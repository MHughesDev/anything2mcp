//! Shared types for Anything2MCP: the vocabularies, checked names,
//! identifiers, values, failures and the tables that declare their codes, findings, the secret wrapper, and the
//! compiled target as data.
//!
//! Everything here is the same whatever kind of system is being integrated.
//! A word with a fixed set of values is an enum whose names are the ones the
//! design documents use; `tests/documents.rs` holds the two together. A word
//! whose values arrive with packs, such as a mode, is a checked name.
//!
//! This crate holds types and the rules that make a value well formed. It
//! does nothing: it reads no clock, no random source, no file, and no
//! network. An identifier is made from a moment and random bytes the caller
//! supplies. The one exception is the hidden module `testing`, built only
//! for tests and under the feature `testing`, which reads a crate's sources
//! for one test other crates keep.
//!
//! The crates this one may depend on are set by the layering table in
//! `docs/reference/technical-reference.md`: none.

mod name;
mod vocabulary;

mod canonical;
mod code_book;
pub mod codes;
mod compiled_target;
mod decimal;
mod digest;
mod effect;
mod evidence;
mod failure;
mod finding;
mod ident;
mod kind;
mod notice;
mod redact;
mod scope;
mod secret;
mod state;
mod value;

/// What the test `every_failure_takes_its_class_from_a_table` of other
/// crates reads their sources with. Not part of the interface.
#[cfg(any(test, feature = "testing"))]
#[doc(hidden)]
pub mod testing;

/// What [`checked_name!`] expands to. Not part of the interface.
#[doc(hidden)]
pub mod __name {
    pub use crate::name::{check_dotted_name, check_name, is_dotted_name, is_name};
    pub use serde;
}

pub use canonical::{CanonicalJson, PackPayload};
pub use code_book::{CodeBook, CodeEntry, Passes, ThenAction};
pub use compiled_target::{
    AddressOverride, ApproveWrites, AuthSetting, COMPILED_TARGET_FORMAT, CaptureLevel, Checkpoint,
    CodeLabel, CompiledAuth, CompiledBinding, CompiledEnvironment, CompiledExpr, CompiledFieldType,
    CompiledIdentity, CompiledInput, CompiledOperation, CompiledOutput, CompiledPipeline,
    CompiledProbe, CompiledRate, CompiledSource, CompiledSurface, CompiledTarget,
    CompiledTargetHeader, CompiledTest, CompiledTool, CredentialUse, DecimalMark, DeclaredProgram,
    DescriptionBy, DriftRisk, EncodedCompiledTarget, EnvironmentIndex, EnvironmentKind, ExprIndex,
    FailsAs, FieldBase, FieldFormat, FieldTypeIndex, InputIndex, OperationIndex, PackRef,
    PackSection, PermissionManifest, PipelineIndex, PipelineLimits, PipelineStep, RecordMatching,
    RunQuestion, SectionIndex, Sensitivity, SignInKind, Stands, Step, StepArg, SurfaceIndex,
    TargetSection, TestKind, ToolAnnotations, ToolIndex, ToolRuns, WhitespaceRule, ZoneRule,
    acknowledgement_may_stand,
};
pub use decimal::{Decimal, Rounding};
pub use digest::{Digest, FieldHasher, InvalidDigest};
pub use effect::{EffectClass, Hazard};
pub use evidence::{Coverage, EvidenceLevel, WriteEvidence};
pub use failure::{
    Failure, FailureClass, FailureCode, NextAction, SAID_CEILING, SAID_LONGEST, SAID_TOO_LONG,
    said_cut, said_hides, said_neutral, said_quoted, said_shown,
};
pub use finding::{
    Address, Finding, FindingBuilder, FindingKind, InvalidFinding, Limitation, LimitationCode,
    ProducedBy, Source, Subject, Timestamp, UnknownReason,
};
pub use ident::{
    AccountName, ApprovalId, AuthProviderName, ControlName, CredentialRef, DecisionId, Environment,
    Epoch, FileKindName, Identity, InvalidId, JobId, LintCode, OperationName, PrincipalId, RunId,
    RunnerId, SessionId, StepKind, SurfaceKind, SurfaceName, TargetName, Verb, Version,
    WorkspaceId,
};
pub use kind::{AccessKind, Mode};
pub use name::{InvalidName, UnknownName, is_kept_for_a_device};
pub use notice::SurfaceNotice;
pub use redact::{
    DigestPurpose, KeyedDigest, NoSecrets, REDACTED, Redact, hmac_sha256, keyed_digest_under,
};
pub use scope::{AddressScope, ScopedHost};
pub use secret::Secret;
pub use state::{
    Availability, BindingStatus, DoctorState, RecordState, RunState, SessionState, SubsystemMove,
    SubsystemState,
};
pub use value::{Date, FieldOf, InvalidValue, Millis, RecordKey, Value, ValueType, Values};
