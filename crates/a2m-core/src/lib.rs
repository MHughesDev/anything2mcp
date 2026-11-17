//! Shared types for Anything2MCP: the vocabularies, checked names,
//! identifiers, values, failures, findings, the secret wrapper, and the
//! compiled plan as data.
//!
//! Everything here is the same whatever kind of system is being integrated.
//! A word with a fixed set of values is an enum whose names are the ones the
//! design documents use; `tests/documents.rs` holds the two together. A word
//! whose values arrive with packs, such as a mode, is a checked name.
//!
//! This crate holds types and the rules that make a value well formed. It
//! does nothing: it reads no clock, no random source, no file, and no
//! network. An identifier is made from a moment and random bytes the caller
//! supplies.
//!
//! The crates this one may depend on are set by the layering table in
//! `docs/reference/technical-reference.md`: none.

mod name;
mod vocabulary;

mod canonical;
mod decimal;
mod digest;
mod effect;
mod evidence;
mod failure;
mod finding;
mod ident;
mod kind;
mod plan;
mod redact;
mod secret;
mod state;
mod value;

/// What [`checked_name!`] expands to. Not part of the interface.
#[doc(hidden)]
pub mod __name {
    pub use crate::name::{check_dotted_name, check_name, is_dotted_name, is_name};
    pub use serde;
}

pub use canonical::{CanonicalJson, PackPayload};
pub use decimal::{Decimal, Rounding};
pub use digest::{Digest, FieldHasher, InvalidDigest};
pub use effect::{EffectClass, Hazard};
pub use evidence::{Coverage, EvidenceLevel};
pub use failure::{Failure, FailureClass, FailureCode};
pub use finding::{
    Address, Finding, FindingBuilder, FindingKind, InvalidFinding, Limitation, LimitationCode,
    ProducedBy, Source, Subject, Timestamp, UnknownReason,
};
pub use ident::{
    AccountName, ApprovalId, ControlName, DecisionId, Environment, Epoch, FileKindName, Identity,
    InvalidId, JobId, LintCode, OperationName, PrincipalId, RunId, RunnerId, SessionId, StepKind,
    SurfaceKind, SurfaceName, TargetName, Verb, Version, WorkspaceId,
};
pub use kind::{Archetype, Mode};
pub use name::{InvalidName, UnknownName};
pub use plan::{
    AddressOverride, BindingPlan, Checkpoint, CodeLabel, EncodedPlan, EnvironmentKind,
    EnvironmentPlan, ExprIndex, FailsAs, FieldBase, FieldTypeIndex, FieldTypePlan, IdentityPlan,
    IdentityStrategy, InputIndex, InputPlan, OperationIndex, OperationPlan, OutputPlan,
    PLAN_FORMAT, PackRef, PackSection, PermissionManifest, PipelineIndex, PipelineLimits,
    PipelinePlan, PipelineStep, Plan, PlanExpr, PlanHeader, RatePlan, SectionIndex, Sensitivity,
    SourcePlan, Step, StepArg, SurfaceIndex, SurfacePlan, TestKind, TestPlan, ToolAnnotations,
    ToolIndex, ToolPlan, ToolRuns, ZoneRule,
};
pub use redact::{
    DigestPurpose, KeyedDigest, NoSecrets, REDACTED, Redact, hmac_sha256, keyed_digest_under,
};
pub use secret::Secret;
pub use state::{
    Availability, BindingStatus, DoctorState, RecordState, RunState, SessionState, SubsystemMove,
    SubsystemState,
};
pub use value::{Date, FieldOf, InvalidValue, Millis, RecordKey, Value, ValueType, Values};
