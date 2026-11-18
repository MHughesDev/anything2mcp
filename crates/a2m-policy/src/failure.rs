//! The failures this crate can produce: one class and one code each.
//!
//! A refusal tells the caller the code and what to ask for. It never holds
//! the content of a rule; the determining rules are kept with the recorded
//! decision, for `explain`.

use a2m_core::{Failure, FailureClass, FailureCode};

pub use crate::codes::{
    APPROVAL_MISSING, APPROVAL_REFUSED, APPROVAL_STALE, DECISION_NOT_FOUND, EFFECT_TOO_HIGH,
    IDENTITY_NOT_ALLOWED, KILL_SWITCH_ON, LIMIT_EXCEEDED, NOT_GRANTED, POLICY_EVALUATION_FAILED,
    POLICY_TESTS_FAILED, PRESENCE_REQUIRED,
};

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
        "Check the engine's status; if no valid policy set is loaded, publish one.",
    )
}

pub(crate) fn presence_required(what: &str) -> Failure {
    policy(
        PRESENCE_REQUIRED,
        format!("Only a person who is present may do this: {what}."),
        "Make the change yourself and pass the user-presence prompt.",
    )
}
