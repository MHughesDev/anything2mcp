//! A stand-in for the policy engine, for the tests of crates that need a
//! write run cleared without opening one. Only with the `testing` feature,
//! which a crate turns on for its tests alone, as a feature of its
//! dev-dependency; no normal build carries it (decision 0145).

use a2m_core::{ApprovalId, DecisionId, Failure, PrincipalId};

use crate::ledger::{Fence, WriteClearance};
use crate::tx::{SqlValue, Tx};

/// How long the stand-in's approvals last: an hour from the transaction's
/// moment.
const WINDOW_MILLIS: i64 = 3_600_000;

/// Records an approval of the run's diff and tool version, by a principal
/// that is not the run's, with a presence check that proves presence, and
/// marks it used in `tx`: what the policy engine's approval and `consume`
/// do. Begin the writes with the clearance in the same `tx`.
///
/// # Errors
///
/// `input`/`not_found` when there is no such run; `policy`/
/// `approval_missing` when the run has no diff yet; the store's failures.
pub fn approve(tx: &mut Tx<'_>, fence: &Fence) -> Result<WriteClearance, Failure> {
    let run = tx.ledger().run(fence.run)?;
    let diff = run
        .diff_hash
        .ok_or_else(|| crate::failure::approval_missing("the run has no diff to approve"))?;
    let id = ApprovalId::from_parts(tx.now(), tx.random()?)?;
    let now = tx.now();
    let expires = now.as_unix_millis().saturating_add(WINDOW_MILLIS);
    let key = tx.shared_policy_key_for_tests();
    tx.policy_tables(&key)?.execute(
        "INSERT INTO approvals
            (id, workspace_id, scope, run_id, diff_hash, tool_version_hash, approver_id, channel,
             presence_check, granted_at, expires_at, used_at)
         VALUES (?1, ?2, 'run', ?3, ?4, ?5, ?6, 'local_prompt', 'os_prompt', ?7, ?8, ?7)",
        &[
            id.to_string().into(),
            run.workspace.to_string().into(),
            run.id.to_string().into(),
            diff.to_string().into(),
            run.target_version.to_string().into(),
            PrincipalId::from_bytes([0xa9; 16]).to_string().into(),
            now.into(),
            SqlValue::Int(expires),
        ],
    )?;
    Ok(WriteClearance::Approval(id))
}

/// Records, in `tx`, an allowed decision that the run may write without a
/// person, for its diff, under a stand-in policy version: what the policy
/// engine's `decide` records for a pre-approval rule.
///
/// # Errors
///
/// As [`approve`].
pub fn pre_approve(tx: &mut Tx<'_>, fence: &Fence) -> Result<WriteClearance, Failure> {
    let run = tx.ledger().run(fence.run)?;
    let diff = run
        .diff_hash
        .ok_or_else(|| crate::failure::approval_missing("the run has no diff to approve"))?;
    let id = DecisionId::from_parts(tx.now(), tx.random()?)?;
    let now = tx.now();
    let key = tx.shared_policy_key_for_tests();
    tx.policy_tables(&key)?.execute(
        "INSERT INTO decisions
            (id, workspace_id, run_id, principal_id, question, allowed, diff_hash, reasons,
             policy_version, decided_at)
         VALUES (?1, ?2, ?3, ?4, 'write_without_person', 1, ?5, '[]', 'testing', ?6)",
        &[
            id.to_string().into(),
            run.workspace.to_string().into(),
            run.id.to_string().into(),
            run.principal.to_string().into(),
            diff.to_string().into(),
            now.into(),
        ],
    )?;
    Ok(WriteClearance::PreApproved(id))
}
