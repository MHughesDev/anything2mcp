# 0381. A run stopped before anything was planned is validated again when resumed

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md). The owner had not read this record when it was merged. (Its number is a placeholder beside the drift-mark record of the same number, [A drift marks the knowledge it was seen in](0380-a-drift-marks-the-knowledge-it-was-seen-in-and-stops-every-binding-that-relies-on-it.md); the lead renumbers both.)

**Date:** 2026-11-24

**Changes:** the run state rules of `a2m-ledger` (`Ledger::set_state` in `ledger.rs`); where the engine moves a resumed run (`Engine::run_job` in `engine.rs`); the state table of [section 208](../spec/2-kernel/208-runs-the-ledger-and-resume.md) as built.

## Context

A write that a drift mark or an edge block stops at its preview is `stopped` while `validating`, before any record is planned and before a diff is stored. Section 217 and the engine's design say such a run is resumed once the cause is gone. It could not be. The ledger let a `stopped` run move only to `running`, `stopped`, `failed`, or `cancelled`, and plans records only in `validating` (`Ledger::plan_records`). The engine moved a run to `validating` only when it had no snapshot, so a resumed run's preview was refused as `ledger_state_unexpected`. A run at `needs_login` already has the move back to `validating`, for the same reason.

## Decision

1. **A `stopped` run may move to `validating` when it has no planned record and no stored diff.** Every other rule stands: a stopped run with a record, in any state, or with a diff, still moves only as before. Whether it has a record is counted from `run_records` in the same transaction as the move.
2. **The engine sends `validating` when it resumes such a run.** A snapshot whose state is `stopped`, with no diff and no records, is treated as a new run for its first move. Its preview then plans its records and stores its diff, and the run rests for approval as any other.

*Instead of:* cancelling the stopped run and starting a new one on the next call. The caller's identical call is deduplicated to the stopped run (ledger rule 6) and told to resume it, so the resume must work. *Instead of:* allowing the move for any stopped run. A run with planned records may have a record in doubt, which reconcile must settle under the rules it already has, and never by planning again.

## Proof

`crates/a2m-ledger/tests/store.rs`: `a_stopped_run_with_nothing_planned_is_validated_again_and_plans_its_records` and `a_stopped_run_with_a_planned_record_is_never_validated_again`. `crates/a2m-engine/tests/faults.rs`: `a_write_stopped_at_its_preview_is_resumed_and_writes_once`, where an edge block stops the preview and the same run, resumed, is previewed, approved, written once, and stored as sent. `crates/a2m-actions/src/tests/drift.rs`, the second test: a write stopped by a drift mark is resumed after a canary lifts the mark and rests for approval. The nine rule tests of `crates/a2m-ledger/tests/rules.rs` pass unchanged.
