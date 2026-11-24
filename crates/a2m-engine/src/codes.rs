//! The failure codes the engine itself produces. Each belongs to exactly
//! one class. Some are also declared by the crates whose work they report,
//! with the same class: `needs_login`, `read_back_differs`,
//! `changed_since_preview`, `approval_stale`, `no_confirmation`, and
//! `engine_degraded`.
//!
//! Whether a `busy` code passes by itself, and why:
//!
//! - `engine_degraded` does not: what degraded the kernel is repaired by a
//!   person, as `anything2mcp doctor run` names it.
//! - `no_binding` does not: a provider is enabled, or a binding's status is
//!   changed, by a person.
//! - `run_time_exceeded` does not: the same run takes as long again; it is
//!   resumed, which keeps what it verified.
//! - `blocked_by_parent` does not: the parent record failed, and is settled
//!   first.
//! - `job_kind_not_built` does not: it is a part not built yet.
//! - `evidence_not_stored` does not: a person frees space in the store.
//! - `no_rate_budget` does: the surface's budget refills.
//! - `control_unanswered` does: the control plane may answer the next call
//!   in time.
//! - `capture_not_kept`, `acknowledgement_not_kept`, and `shadow_not_kept`
//!   do not: the control plane does not keep them, and does not start to.
//! - `shadow_time_exceeded` does: a later call runs the shadow binding
//!   again, and it may end in time.

use a2m_core::{CodeEntry, Failure};

a2m_core::failure_codes! {
    PLAN_UNVERIFIED = Policy, "plan_unverified",
        meaning "The plan's bytes do not give the hash the job names.",
        next "Publish the target again from its stored sources.";
    ENGINE_DEGRADED = Busy, "engine_degraded",
        meaning "A write was asked of a kernel that takes reads only, or the kernel has stopped.",
        next "Run `anything2mcp doctor run`, repair what it names, then call again.",
        passes not_by_itself;
    NO_BINDING = Busy, "no_binding",
        meaning "No binding qualifies, for a reason that is not a session, an edge block, or drift.",
        next "Check the binding's status and its provider with `anything2mcp engine status`, and enable a provider that drives a mode of the surface.",
        passes not_by_itself;
    EXPECT_FAILED = UnknownVariant, "expect_failed",
        meaning "An `expect` does not hold.",
        next "Look at the record in the system; the tool runs only when its expectation holds.";
    JOB_INVALID = Input, "job_invalid",
        meaning "The job names a tool, an environment, or a surface the plan does not hold, or a job of a kind the tool cannot run as.",
        next "Submit the job again for a tool, environment, and surface the published plan holds.";
    INPUT_INVALID = Input, "input_invalid",
        meaning "An input is missing, is not declared, or is not of its type.",
        next "Call the tool with every input it declares, each of its type.";
    RECORD_LIMIT = Policy, "record_limit_exceeded",
        meaning "The run would touch more records than its limit allows.",
        next "Split the source, or raise the pipeline's limit and publish again.";
    FIELD_NOT_READ_BACK = UnknownVariant, "field_not_read_back",
        meaning "Reading a record gave no value for a field the operation writes.",
        next "Make the binding's read-back return every field the operation writes.";
    RUN_TIME_EXCEEDED = Busy, "run_time_exceeded",
        meaning "The run passed the longest time its pipeline allows.",
        next "Resume the run; what it verified stays verified.",
        passes not_by_itself;
    NEEDS_LOGIN = SessionExpired, "needs_login",
        meaning "The session ended and could not be had again.",
        next "Sign in with `anything2mcp session login`, then resume the run, which reconciles any write it began before it writes again.";
    EDGE_BLOCKED = EdgeBlocked, "edge_blocked",
        meaning "The surface is blocked at the edge.",
        next "Wait for the block to end; the engine does not go around it.";
    BINDING_DRIFTED = Drift, "binding_drifted",
        meaning "Every binding that could carry the call out has drifted.",
        next "Repair the binding, verify it, and publish the target again.";
    READ_BACK_DIFFERS = Mismatch, "read_back_differs",
        meaning "The value read back differs from the value intended.",
        next "Compare the values in the evidence, then cancel the run or settle the record by hand.";
    CHANGED_SINCE_PREVIEW = Stale, "changed_since_preview",
        meaning "A record no longer holds what the preview saw.",
        next "Preview the run again and approve the new diff.";
    CANNOT_RECONCILE = PossibleCommit, "cannot_reconcile",
        meaning "A record in doubt cannot be read, so a person must settle it.",
        next "Look at the system, then cancel the run or settle the record by hand.";
    NO_CONFIRMATION = PossibleCommit, "no_confirmation",
        meaning "A write was sent and nothing says whether it happened.",
        next "Reconcile the record: read it back before anything is written again.";
    NO_READ_BACK = UnknownVariant, "no_read_back",
        meaning "A binding has no steps that read the record, so nothing it writes can be verified.",
        next "Give the binding a read-back, or declare the operation `irreversible`, and publish the target again.";
    OUTPUT_UNEXPECTED = UnknownVariant, "output_unexpected",
        meaning "A step returned a value the plan gives no place to.",
        next "Compare the operation's outputs in the target files with what the system returns.";
    BLOCKED_BY_PARENT = Busy, "blocked_by_parent",
        meaning "A record waits on a parent record that failed.",
        next "Settle the earlier record, then resume the run.",
        passes not_by_itself;
    SANDBOX_READ_ONLY = Policy, "sandbox_read_only",
        meaning "A write was asked for where nothing may be written.",
        next "Declare the record under the environment's test records, or try a read.";
    JOB_KIND_NOT_BUILT = Busy, "job_kind_not_built",
        meaning "The job is of a kind this engine does not carry out yet.",
        next "Do the work as jobs of a kind the engine carries out, such as one operation after the other.",
        passes not_by_itself;
    EVIDENCE_NOT_STORED = Busy, "evidence_not_stored",
        meaning "Evidence could not be stored, so nothing can be marked verified.",
        next "Free space in the evidence store, then resume the run.",
        passes not_by_itself;
    APPROVAL_STALE = Policy, "approval_stale",
        meaning "The approval is expired, used, declined, revoked, or for another diff.",
        next "Preview the run again and have a person approve the new diff.";
    PREPARE_TIMED_OUT = Transient, "prepare_timed_out",
        meaning "A driver was not ready within the time preparing may take.",
        next "Call the tool again.";
    NO_RATE_BUDGET = Busy, "no_rate_budget",
        meaning "No rate budget was granted for a surface.",
        next "Call the tool again later.",
        passes by_itself;
    NO_TOKEN = Policy, "intent_token_missing",
        meaning "An intent was stored and no token came back for it.",
        next "Report this as a fault in the engine; nothing was sent.";
    COMMIT_SKIPPED = UnknownVariant, "commit_skipped",
        meaning "A binding ended without sending its write.",
        next "Correct the binding, or the step's condition, and publish the target again.";
    CONTROL_UNANSWERED = Busy, "control_unanswered",
        meaning "A call to the control plane was not answered within the time the engine waits for one.",
        next "See `anything2mcp engine status`, then resume the run.",
        passes by_itself;
    DRIVER_PANICKED = UnknownVariant, "driver_panicked",
        meaning "A provider's code panicked. The engine goes on; the step does not.",
        next "Report this to the provider's author.";
    CONDITION_NOT_BOOLEAN = UnknownVariant, "condition_not_boolean",
        meaning "A condition gave a value that is neither true nor false.",
        next "Compare the condition in the target files with what the system returns.";
    ARGUMENT_MASKED = UnknownVariant, "argument_masked",
        meaning "An argument of a step holds the mask of a value the redactor removed.",
        next "Change the step so that it does not take a value the system shows as a registered secret.";
    CAPTURE_NOT_KEPT = Busy, "capture_not_kept",
        meaning "The control plane keeps no captures; the capture was dropped.",
        next "Run on an engine that keeps captures if the capture is needed; the run goes on without it.",
        passes not_by_itself;
    ACKNOWLEDGEMENT_NOT_KEPT = Busy, "acknowledgement_not_kept",
        meaning "The control plane cannot end a record on the system's acknowledgement alone; the record stays `acted`, in doubt.",
        next "Look at the system and settle the record; the run stops until it is settled.",
        passes not_by_itself;
    BINDING_CHANGED = Stale, "binding_changed",
        meaning "The binding a test or a trial named is no longer the one at its place: it moved, changed its mode, or changed what it does.",
        next "Run the test or the trial again against the drafts as they are now.";
    SHADOW_NOT_KEPT = Busy, "shadow_not_kept",
        meaning "The control plane keeps no shadow comparisons, so no binding is run in shadow.",
        next "Compare a binding in shadow on an engine that keeps the comparisons; the call's result is the verified binding's.",
        passes not_by_itself;
    SHADOW_TIME_EXCEEDED = Busy, "shadow_time_exceeded",
        meaning "A shadow binding did not end within the time a shadow may take; it was cancelled and counted as not compared.",
        next "Call again; the shadow binding is compared on a later call.",
        passes by_itself;
}

/// A failure of the code of `row`, in the class its table gives it.
pub(crate) fn fail(row: &CodeEntry, message: impl Into<String>, next_step: &str) -> Failure {
    row.failure(message, next_step)
}

pub(crate) fn job_invalid(what: impl Into<String>) -> Failure {
    fail(
        &row::JOB_INVALID,
        what,
        "Submit the job again for a tool, environment, and surface the published plan holds.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// No source of this crate gives a failure a class of its own: each is
    /// made from a row of a table, which gives it its class, or from what
    /// a step's plan says it fails as. `tool.rs` remakes a recorded
    /// failure, or one it adds to, in the class it already has, in two
    /// places: a failed record that settles the run, and a failure that
    /// shows what was recorded. A third place fails this test.
    #[test]
    fn every_failure_takes_its_class_from_a_table() {
        let found = a2m_core::testing::classes_of_their_own(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &[("tool.rs", 2)],
        )
        .unwrap();
        assert!(
            found.is_empty(),
            "made with a class of their own: {found:#?}"
        );
        for row in TABLE {
            let made = fail(row, "A case.", row.next);
            assert_eq!((made.class(), made.code().as_str()), (row.class, row.code));
        }
    }
}
