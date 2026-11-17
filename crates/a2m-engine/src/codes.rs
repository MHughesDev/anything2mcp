//! The failure codes the engine itself produces. Each belongs to exactly
//! one class.

use a2m_core::{Failure, FailureClass, FailureCode};

/// `policy`: the plan's bytes do not give the hash the job names.
pub const PLAN_UNVERIFIED: FailureCode = FailureCode::fixed("plan_unverified");
/// `busy`: a write was asked of a kernel that takes reads only.
pub const ENGINE_DEGRADED: FailureCode = FailureCode::fixed("engine_degraded");
/// `busy`: no binding qualifies, for a reason that is not a session, an
/// edge block, or drift.
pub const NO_BINDING: FailureCode = FailureCode::fixed("no_binding");
/// `unknown_variant`: an `expect` does not hold.
pub const EXPECT_FAILED: FailureCode = FailureCode::fixed("expect_failed");
/// `input`: the job names a tool, an environment, or a surface the plan
/// does not hold, or a job of a kind the tool cannot run as.
pub const JOB_INVALID: FailureCode = FailureCode::fixed("job_invalid");
/// `input`: an input is missing, is not declared, or is not of its type.
pub const INPUT_INVALID: FailureCode = FailureCode::fixed("input_invalid");
/// `policy`: the run would touch more records than its limit allows.
pub const RECORD_LIMIT: FailureCode = FailureCode::fixed("record_limit_exceeded");
/// `transient`: a step did not end within its time limit, and nothing it
/// sent can have changed the system.
pub const STEP_TIMED_OUT: FailureCode = FailureCode::fixed("step_timed_out");
/// `busy`: the run passed the longest time its pipeline allows.
pub const RUN_TIME_EXCEEDED: FailureCode = FailureCode::fixed("run_time_exceeded");
/// `session_expired`: the session ended and could not be had again.
pub const NEEDS_LOGIN: FailureCode = FailureCode::fixed("needs_login");
/// `edge_blocked`: the surface is blocked at the edge.
pub const EDGE_BLOCKED: FailureCode = FailureCode::fixed("edge_blocked");
/// `drift`: every binding that could carry the call out has drifted.
pub const BINDING_DRIFTED: FailureCode = FailureCode::fixed("binding_drifted");
/// `mismatch`: the value read back differs from the value intended.
pub const READ_BACK_DIFFERS: FailureCode = FailureCode::fixed("read_back_differs");
/// `stale`: a record no longer holds what the preview saw.
pub const CHANGED_SINCE_PREVIEW: FailureCode = FailureCode::fixed("changed_since_preview");
/// `possible_commit`: a lookup found more than one record.
pub const AMBIGUOUS_MATCH: FailureCode = FailureCode::fixed("ambiguous_match");
/// `possible_commit`: a record in doubt cannot be read, so a person must
/// settle it.
pub const CANNOT_RECONCILE: FailureCode = FailureCode::fixed("cannot_reconcile");
/// `possible_commit`: a write was sent and the system's answer is not known.
pub const NO_CONFIRMATION: FailureCode = FailureCode::fixed("no_confirmation");
/// `unknown_variant`: a binding has no steps that read the record, so
/// nothing it writes can be verified.
pub const NO_READ_BACK: FailureCode = FailureCode::fixed("no_read_back");
/// `unknown_variant`: a step returned a value the plan gives no place to.
pub const OUTPUT_UNEXPECTED: FailureCode = FailureCode::fixed("output_unexpected");
/// `busy`: a record waits on a parent record that failed.
pub const BLOCKED_BY_PARENT: FailureCode = FailureCode::fixed("blocked_by_parent");
/// `policy`: a write was asked for where nothing may be written.
pub const SANDBOX_READ_ONLY: FailureCode = FailureCode::fixed("sandbox_read_only");
/// `busy`: the job is of a kind this engine does not carry out yet.
pub const JOB_KIND_NOT_BUILT: FailureCode = FailureCode::fixed("job_kind_not_built");
/// `busy`: evidence could not be stored, so nothing can be marked verified.
pub const EVIDENCE_NOT_STORED: FailureCode = FailureCode::fixed("evidence_not_stored");

/// Every code of this crate with its class, for the test that holds them to
/// the technical reference.
pub const ALL: &[(FailureClass, &str)] = &[
    (FailureClass::Policy, "plan_unverified"),
    (FailureClass::Busy, "engine_degraded"),
    (FailureClass::Busy, "no_binding"),
    (FailureClass::UnknownVariant, "expect_failed"),
    (FailureClass::Input, "job_invalid"),
    (FailureClass::Input, "input_invalid"),
    (FailureClass::Policy, "record_limit_exceeded"),
    (FailureClass::Transient, "step_timed_out"),
    (FailureClass::Busy, "run_time_exceeded"),
    (FailureClass::SessionExpired, "needs_login"),
    (FailureClass::EdgeBlocked, "edge_blocked"),
    (FailureClass::Drift, "binding_drifted"),
    (FailureClass::Mismatch, "read_back_differs"),
    (FailureClass::Stale, "changed_since_preview"),
    (FailureClass::PossibleCommit, "ambiguous_match"),
    (FailureClass::PossibleCommit, "cannot_reconcile"),
    (FailureClass::PossibleCommit, "no_confirmation"),
    (FailureClass::UnknownVariant, "no_read_back"),
    (FailureClass::UnknownVariant, "output_unexpected"),
    (FailureClass::Busy, "blocked_by_parent"),
    (FailureClass::Policy, "sandbox_read_only"),
    (FailureClass::Busy, "job_kind_not_built"),
    (FailureClass::Busy, "evidence_not_stored"),
];

pub(crate) fn fail(
    class: FailureClass,
    code: FailureCode,
    message: impl Into<String>,
    next_step: &str,
) -> Failure {
    Failure::new(class, code, message.into(), next_step.to_owned())
}

pub(crate) fn job_invalid(what: impl Into<String>) -> Failure {
    fail(
        FailureClass::Input,
        JOB_INVALID,
        what,
        "Submit the job again for a tool, environment, and surface the published plan holds.",
    )
}
