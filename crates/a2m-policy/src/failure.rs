//! The failures this crate can produce: one class and one code each.
//!
//! A refusal tells the caller the code and what to ask for. It never holds
//! the content of a rule; the determining rules are kept with the recorded
//! decision, for `explain`.

use a2m_core::{Failure, FailureClass, FailureCode};

/// No permit applies to the caller, the tool, or the action.
pub const NOT_GRANTED: FailureCode = FailureCode::fixed("not_granted");
/// The run may not act as the identity, including every attempt to use
/// another person's session.
pub const IDENTITY_NOT_ALLOWED: FailureCode = FailureCode::fixed("identity_not_allowed");
/// A grant exists, with a lower highest effect class.
pub const EFFECT_TOO_HIGH: FailureCode = FailureCode::fixed("effect_too_high");
/// A write reached the write phase with no usable approval.
pub const APPROVAL_MISSING: FailureCode = FailureCode::fixed("approval_missing");
/// The approval is expired, used, declined, revoked, or for another diff.
pub const APPROVAL_STALE: FailureCode = FailureCode::fixed("approval_stale");
/// The person or the caller may not approve this run.
pub const APPROVAL_REFUSED: FailureCode = FailureCode::fixed("approval_refused");
/// Records per run, or a quota.
pub const LIMIT_EXCEEDED: FailureCode = FailureCode::fixed("limit_exceeded");
/// A kill switch covers the request.
pub const KILL_SWITCH_ON: FailureCode = FailureCode::fixed("kill_switch_on");
/// An error while deciding, or no valid policy set is loaded.
pub const POLICY_EVALUATION_FAILED: FailureCode = FailureCode::fixed("policy_evaluation_failed");
/// A change to a protected setting that only a person, present, may make.
pub const PRESENCE_REQUIRED: FailureCode = FailureCode::fixed("presence_required");
/// A policy set whose own test table does not pass.
pub const POLICY_TESTS_FAILED: FailureCode = FailureCode::fixed("policy_tests_failed");
/// No decision with that identifier is recorded.
pub const DECISION_NOT_FOUND: FailureCode = FailureCode::fixed("decision_not_found");

pub(crate) fn policy(
    code: FailureCode,
    message: impl Into<String>,
    next: impl Into<String>,
) -> Failure {
    Failure::new(FailureClass::Policy, code, message, next)
}

pub(crate) fn invalid(message: impl Into<String>, next: impl Into<String>) -> Failure {
    Failure::new(
        FailureClass::Input,
        FailureCode::SCHEMA_INVALID,
        message,
        next,
    )
}

pub(crate) fn not_granted() -> Failure {
    policy(
        NOT_GRANTED,
        "No grant allows this caller to do this.",
        "Ask an administrator for a grant that covers it.",
    )
}

pub(crate) fn effect_too_high() -> Failure {
    policy(
        EFFECT_TOO_HIGH,
        "The caller's grant covers this tool, but not at this effect class.",
        "Ask an administrator to raise the grant's highest effect class.",
    )
}

pub(crate) fn identity_not_allowed() -> Failure {
    policy(
        IDENTITY_NOT_ALLOWED,
        "The run may not act as this identity.",
        "Use a toolset whose identity is the caller's own, or ask the person to delegate.",
    )
}

pub(crate) fn approval_refused() -> Failure {
    policy(
        APPROVAL_REFUSED,
        "This approval cannot be recorded.",
        "Have a person who may approve this run approve it on the approval surface.",
    )
}

pub(crate) fn approval_missing() -> Failure {
    policy(
        APPROVAL_MISSING,
        "The write has no usable approval.",
        "Have a person approve the run's diff, then resume the run.",
    )
}

pub(crate) fn approval_stale(why: &str) -> Failure {
    policy(
        APPROVAL_STALE,
        format!("The approval cannot be used: {why}."),
        "Preview the run again and have a person approve the new diff.",
    )
}

pub(crate) fn limit_exceeded(what: &str) -> Failure {
    policy(
        LIMIT_EXCEEDED,
        format!("The call is over a limit: {what}."),
        "Send fewer records in one run, or wait for the next period.",
    )
}

pub(crate) fn kill_switch_on() -> Failure {
    policy(
        KILL_SWITCH_ON,
        "A kill switch stops this work.",
        "Ask an administrator why the kill switch is set; it must be released before this can run.",
    )
}

pub(crate) fn evaluation_failed() -> Failure {
    policy(
        POLICY_EVALUATION_FAILED,
        "The policy engine could not decide, so the answer is no.",
        "Run `doctor`; if no valid policy set is loaded, publish one.",
    )
}

pub(crate) fn presence_required(what: &str) -> Failure {
    policy(
        PRESENCE_REQUIRED,
        format!("Only a person who is present may do this: {what}."),
        "Make the change yourself and pass the user-presence prompt.",
    )
}
