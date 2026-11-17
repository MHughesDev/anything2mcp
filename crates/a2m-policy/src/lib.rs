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

mod failure;
mod model;
mod rules;
mod set;

pub use failure::{
    APPROVAL_MISSING, APPROVAL_REFUSED, APPROVAL_STALE, DECISION_NOT_FOUND, EFFECT_TOO_HIGH,
    IDENTITY_NOT_ALLOWED, KILL_SWITCH_ON, LIMIT_EXCEEDED, NOT_GRANTED, POLICY_EVALUATION_FAILED,
    POLICY_TESTS_FAILED, PRESENCE_REQUIRED,
};
pub use model::{
    ActingFor, ActionName, ActionRef, Context, GroupName, IdentityMode, ModuleName, OptionName,
    PrincipalKind, PrincipalRef, Question, Request, Resource, Role, RuleId, RunRef, ToolRef,
    ToolsetName,
};
pub use rules::Answer;
pub use set::{
    ApproveWrites, BuiltIn, CONDITION_NAMES, CandidateSet, Grant, LabelGrant, OptionGrant,
    PolicyLayers, PolicyTest, PreApproval, Profile, Quota, RoleBinding, SET_FORMAT, Scope, Settings,
    Subject, TestFailure, TestReport,
};
