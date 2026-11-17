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
/// `unknown_variant`: reading a record gave no value for a field the
/// operation writes.
pub const FIELD_NOT_READ_BACK: FailureCode = FailureCode::fixed("field_not_read_back");
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

/// `transient`: a driver was not ready within the time preparing may take.
pub const PREPARE_TIMED_OUT: FailureCode = FailureCode::fixed("prepare_timed_out");
/// `busy`: no rate budget was granted for a surface.
pub const NO_RATE_BUDGET: FailureCode = FailureCode::fixed("no_rate_budget");
/// `policy`: an intent was stored and no token came back for it.
pub const NO_TOKEN: FailureCode = FailureCode::fixed("intent_token_missing");
/// `unknown_variant`: a binding ended without sending its write.
pub const COMMIT_SKIPPED: FailureCode = FailureCode::fixed("commit_skipped");

/// `busy`: a call to the control plane was not answered within the time the
/// engine waits for one.
pub const CONTROL_UNANSWERED: FailureCode = FailureCode::fixed("control_unanswered");
/// `unknown_variant`: a provider's code panicked. The engine goes on; the
/// step does not.
pub const DRIVER_PANICKED: FailureCode = FailureCode::fixed("driver_panicked");

/// `unknown_variant`: a condition gave a value that is neither true nor
/// false.
pub const CONDITION_NOT_BOOLEAN: FailureCode = FailureCode::fixed("condition_not_boolean");
/// `unknown_variant`: an argument of a step holds the mask of a value the
/// redactor removed.
pub const ARGUMENT_MASKED: FailureCode = FailureCode::fixed("argument_masked");

/// `busy`: the control plane keeps no captures; the capture was dropped.
pub const CAPTURE_NOT_KEPT: FailureCode = FailureCode::fixed("capture_not_kept");
/// `busy`: the control plane cannot end a record on the system's
/// acknowledgement alone; the record stays `acted`, in doubt.
pub const ACKNOWLEDGEMENT_NOT_KEPT: FailureCode = FailureCode::fixed("acknowledgement_not_kept");

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
    (FailureClass::UnknownVariant, "field_not_read_back"),
    (FailureClass::Busy, "run_time_exceeded"),
    (FailureClass::SessionExpired, "needs_login"),
    (FailureClass::EdgeBlocked, "edge_blocked"),
    (FailureClass::Drift, "binding_drifted"),
    (FailureClass::Mismatch, "read_back_differs"),
    (FailureClass::Stale, "changed_since_preview"),
    (FailureClass::PossibleCommit, "cannot_reconcile"),
    (FailureClass::PossibleCommit, "no_confirmation"),
    (FailureClass::UnknownVariant, "no_read_back"),
    (FailureClass::UnknownVariant, "output_unexpected"),
    (FailureClass::Busy, "blocked_by_parent"),
    (FailureClass::Policy, "sandbox_read_only"),
    (FailureClass::Busy, "job_kind_not_built"),
    (FailureClass::Busy, "evidence_not_stored"),
    (FailureClass::Transient, "prepare_timed_out"),
    (FailureClass::Busy, "no_rate_budget"),
    (FailureClass::Policy, "intent_token_missing"),
    (FailureClass::UnknownVariant, "commit_skipped"),
    (FailureClass::Busy, "control_unanswered"),
    (FailureClass::UnknownVariant, "driver_panicked"),
    (FailureClass::UnknownVariant, "condition_not_boolean"),
    (FailureClass::UnknownVariant, "argument_masked"),
    (FailureClass::Busy, "capture_not_kept"),
    (FailureClass::Busy, "acknowledgement_not_kept"),
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
