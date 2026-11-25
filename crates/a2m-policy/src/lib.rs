//! The policy engine: the one decider.
//!
//! Every question of the form "may this happen?" is answered here with a
//! plain allow or deny, and every answer is recorded with the rules that
//! made it. The decision core is this crate's own evaluator over typed
//! rules (decision 0082): a request is denied unless a permit applies, and
//! one matching forbid denies it whatever permits apply.
//!
//! The crates this one may depend on are set by the layering table in
//! `docs/reference/technical-reference.md`: layers 1 to 3.

mod approval;
pub mod codes;
mod failure;
mod model;
mod policy;
mod rules;
mod set;
mod store;
mod toolset;

pub use approval::{ApprovalSurface, Approved, Channel, Declined, PresenceCheck};
pub use policy::{
    Decision, Explanation, KillScope, KillSwitch, LayerChange, LimitCheck, Outcome, Policy,
    PolicyHealth, PublishProof, PublishReview, RuleExplained, VersionInfo,
};

pub use failure::{
    APPROVAL_MISSING, APPROVAL_REFUSED, APPROVAL_STALE, DECISION_NOT_FOUND, EFFECT_TOO_HIGH,
    IDENTITY_NOT_ALLOWED, KILL_SWITCH_ON, LIMIT_EXCEEDED, NOT_GRANTED, POLICY_EVALUATION_FAILED,
    POLICY_TESTS_FAILED, PRESENCE_REQUIRED, SECOND_ADMIN_REQUIRED,
};
pub use model::{
    ActingFor, ActionName, ActionRef, Context, GroupName, IdentityKind, ModuleName, OptionName,
    PrincipalKind, PrincipalRef, Question, Request, Resource, Role, RuleId, RunRef, ToolRef,
    ToolsetName,
};
pub use rules::{Answer, RuleKind};
pub use set::{
    ApproveWrites, BuiltIn, CONDITION_NAMES, CandidateSet, DeploymentShape, Grant, LabelGrant,
    OptionGrant, PolicyLayers, PolicyTest, PreApproval, Quota, RoleBinding, SET_FORMAT, Scope,
    Settings, Subject, TestFailure, TestReport,
};
pub use toolset::{RoleGrant, Toolset};
