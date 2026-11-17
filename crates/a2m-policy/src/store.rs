//! This crate's tables, through the store's transaction. Every statement
//! here names only tables `a2m-policy` owns, and the store refuses any other.

use a2m_core::{Digest, Failure, PrincipalId, Timestamp};
use a2m_ledger::{PolicyKey, Row, SqlValue, Tables, Tx};

use crate::failure;
use crate::model::Request;
use crate::rules::{Answer, Verdict};
use crate::set::CandidateSet;

/// The policy engine's tables in `tx`, with the key the store handed it.
fn tables<'t, 'a>(tx: &'t mut Tx<'a>, key: &PolicyKey) -> Result<Tables<'t, 'a>, Failure> {
    tx.policy_tables(key)
}

/// A stored policy version, as text.
pub(crate) struct StoredVersion {
    pub(crate) version: String,
    pub(crate) layers: String,
    pub(crate) tests: String,
    pub(crate) built_in: bool,
}

/// The version of the newest policy set, if any is stored.
pub(crate) fn newest_version(tx: &mut Tx<'_>, key: &PolicyKey) -> Result<Option<String>, Failure> {
    let row = tables(tx, key)?.query_first(
        "SELECT version FROM policy_versions ORDER BY seq DESC LIMIT 1",
        &[],
    )?;
    row.map(|row| row.text(0).map(str::to_owned)).transpose()
}

pub(crate) fn version(
    tx: &mut Tx<'_>,
    key: &PolicyKey,
    version: &str,
) -> Result<Option<StoredVersion>, Failure> {
    let row = tables(tx, key)?.query_first(
        "SELECT version, layers, tests, published_by FROM policy_versions WHERE version = ?1
         ORDER BY seq DESC LIMIT 1",
        &[SqlValue::from(version)],
    )?;
    row.map(|row| {
        Ok(StoredVersion {
            version: row.text(0)?.to_owned(),
            layers: row.text(1)?.to_owned(),
            tests: row.text(2)?.to_owned(),
            built_in: row.opt_text(3)?.is_none(),
        })
    })
    .transpose()
}

/// Stores a version as the newest. Storing the version that is already the
/// newest changes nothing.
pub(crate) fn insert_version(
    tx: &mut Tx<'_>,
    key: &PolicyKey,
    set: &CandidateSet,
    guardrails: Digest,
    published_by: Option<PrincipalId>,
    presence: Option<&'static str>,
) -> Result<(), Failure> {
    let version = set.version().to_string();
    if newest_version(tx, key)?.as_deref() == Some(version.as_str()) {
        return Ok(());
    }
    let now = tx.now();
    tables(tx, key)?.execute(
        "INSERT INTO policy_versions
            (seq, version, format, guardrails, layers, tests, published_by, presence_check, published_at)
         VALUES ((SELECT COALESCE(MAX(seq), 0) + 1 FROM policy_versions),
                 ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        &[
            SqlValue::from(version),
            SqlValue::from(crate::set::SET_FORMAT),
            SqlValue::from(guardrails.to_string()),
            SqlValue::from(set.layers_text.clone()),
            SqlValue::from(set.tests_text.clone()),
            SqlValue::from(published_by.map(|by| by.to_string())),
            SqlValue::from(presence),
            SqlValue::from(now),
        ],
    )?;
    Ok(())
}

/// Records one decision.
pub(crate) fn insert_decision(
    tx: &mut Tx<'_>,
    key: &PolicyKey,
    id: &str,
    request: &Request,
    request_text: &str,
    verdict: &Verdict,
    version: Option<Digest>,
) -> Result<(), Failure> {
    let reasons =
        serde_json::to_string(&verdict.rules).map_err(|_| failure::evaluation_failed())?;
    let now = tx.now();
    tables(tx, key)?.execute(
        "INSERT INTO decisions
            (id, workspace_id, run_id, principal_id, question, allowed, diff_hash, reasons,
             policy_version, decided_at, resource, request, refusal_code)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        &[
            SqlValue::from(id),
            SqlValue::from(request.context.workspace.to_string()),
            SqlValue::from(request.run.map(|run| run.to_string())),
            SqlValue::from(request.principal.id.to_string()),
            SqlValue::from(request.question.as_str()),
            SqlValue::from(verdict.answer == Answer::Allow),
            SqlValue::from(request.diff_hash.map(|hash| hash.to_string())),
            SqlValue::from(reasons),
            SqlValue::from(version.map(|version| version.to_string())),
            SqlValue::from(now),
            SqlValue::from(request.resource.label()),
            SqlValue::from(request_text),
            SqlValue::from(
                verdict
                    .refusal
                    .as_ref()
                    .map(|refusal| refusal.code().as_str().to_owned()),
            ),
        ],
    )?;
    Ok(())
}

/// A recorded decision, read back.
pub(crate) struct StoredDecision {
    pub(crate) principal: String,
    pub(crate) question: String,
    pub(crate) allowed: bool,
    pub(crate) reasons: Vec<String>,
    pub(crate) version: Option<String>,
    pub(crate) decided_at: Timestamp,
    pub(crate) resource: Option<String>,
    pub(crate) refusal_code: Option<String>,
    pub(crate) workspace: String,
}

pub(crate) fn decision(
    tx: &mut Tx<'_>,
    key: &PolicyKey,
    id: &str,
) -> Result<Option<StoredDecision>, Failure> {
    let row = tables(tx, key)?.query_first(
        "SELECT principal_id, question, allowed, reasons, policy_version, decided_at, resource,
                refusal_code, workspace_id
         FROM decisions WHERE id = ?1",
        &[SqlValue::from(id)],
    )?;
    row.map(|row| {
        Ok(StoredDecision {
            principal: row.text(0)?.to_owned(),
            question: row.text(1)?.to_owned(),
            allowed: row.int(2)? == 1,
            reasons: serde_json::from_str(row.text(3)?).unwrap_or_default(),
            version: row.opt_text(4)?.map(str::to_owned),
            decided_at: Timestamp::from_unix_millis(row.int(5)?),
            resource: row.opt_text(6)?.map(str::to_owned),
            refusal_code: row.opt_text(7)?.map(str::to_owned),
            workspace: row.text(8)?.to_owned(),
        })
    })
    .transpose()
}

/// The kill switches that are on: scope and name.
pub(crate) fn switches_on(
    tx: &mut Tx<'_>,
    key: &PolicyKey,
) -> Result<Vec<(String, String)>, Failure> {
    tables(tx, key)?
        .query(
            "SELECT scope, name FROM kill_switches WHERE is_on = 1 ORDER BY scope, name",
            &[],
        )?
        .iter()
        .map(|row| Ok((row.text(0)?.to_owned(), row.text(1)?.to_owned())))
        .collect()
}

pub(crate) fn kill_epoch(tx: &mut Tx<'_>, key: &PolicyKey) -> Result<u64, Failure> {
    let row =
        tables(tx, key)?.query_first("SELECT COALESCE(MAX(epoch), 0) FROM kill_switches", &[])?;
    let epoch = row.as_ref().map(|row| row.int(0)).transpose()?.unwrap_or(0);
    Ok(u64::try_from(epoch).unwrap_or(0))
}

pub(crate) fn set_switch(
    tx: &mut Tx<'_>,
    key: &PolicyKey,
    scope: &'static str,
    name: String,
    on: bool,
    by: PrincipalId,
    presence: Option<&'static str>,
) -> Result<u64, Failure> {
    let epoch = kill_epoch(tx, key)?.saturating_add(1);
    let now = tx.now();
    tables(tx, key)?.execute(
        "INSERT INTO kill_switches (scope, name, is_on, epoch, set_by, presence_check, set_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT (scope, name) DO UPDATE SET
            is_on = excluded.is_on, epoch = excluded.epoch, set_by = excluded.set_by,
            presence_check = excluded.presence_check, set_at = excluded.set_at",
        &[
            SqlValue::from(scope),
            SqlValue::from(name),
            SqlValue::from(on),
            SqlValue::from(i64::try_from(epoch).unwrap_or(i64::MAX)),
            SqlValue::from(by.to_string()),
            SqlValue::from(presence),
            SqlValue::from(now),
        ],
    )?;
    Ok(epoch)
}

/// Counts one call in the period that holds the transaction's moment, and
/// gives the count including it. Earlier periods of the same counter are
/// removed.
pub(crate) fn count_call(
    tx: &mut Tx<'_>,
    key: &PolicyKey,
    workspace: String,
    principal: String,
    period_minutes: u32,
) -> Result<i64, Failure> {
    let period_millis = i64::from(period_minutes).saturating_mul(60_000);
    let now = tx.now().as_unix_millis();
    let start = now - now.rem_euclid(period_millis);
    let counter = [
        SqlValue::from(workspace),
        SqlValue::from(principal),
        SqlValue::from(period_minutes),
        SqlValue::from(start),
    ];
    let mut tables = tables(tx, key)?;
    tables.execute(
        "DELETE FROM quota_counters
         WHERE workspace_id = ?1 AND principal_id = ?2 AND period_minutes = ?3
           AND period_start < ?4",
        &counter,
    )?;
    tables.execute(
        "INSERT INTO quota_counters (workspace_id, principal_id, period_minutes, period_start, calls)
         VALUES (?1, ?2, ?3, ?4, 1)
         ON CONFLICT (workspace_id, principal_id, period_minutes, period_start)
         DO UPDATE SET calls = calls + 1",
        &counter,
    )?;
    let row = tables.query_first(
        "SELECT calls FROM quota_counters
         WHERE workspace_id = ?1 AND principal_id = ?2 AND period_minutes = ?3
           AND period_start = ?4",
        &counter,
    )?;
    row.as_ref().map_or(Ok(0), |row| row.int(0))
}

/// An approval row of one run.
pub(crate) struct StoredApproval {
    pub(crate) id: String,
    pub(crate) diff_hash: Option<String>,
    pub(crate) expires_at: Option<i64>,
    pub(crate) used_at: Option<i64>,
    pub(crate) revoked_at: Option<i64>,
    pub(crate) approver: String,
}

fn approval_of(row: &Row) -> Result<StoredApproval, Failure> {
    Ok(StoredApproval {
        id: row.text(0)?.to_owned(),
        diff_hash: row.opt_text(1)?.map(str::to_owned),
        expires_at: row.opt_int(2)?,
        used_at: row.opt_int(3)?,
        revoked_at: row.opt_int(4)?,
        approver: row.text(5)?.to_owned(),
    })
}

/// The approvals recorded for a run, newest first.
pub(crate) fn approvals_of(
    tx: &mut Tx<'_>,
    key: &PolicyKey,
    run: &str,
) -> Result<Vec<StoredApproval>, Failure> {
    tables(tx, key)?
        .query(
            "SELECT id, diff_hash, expires_at, used_at, revoked_at, approver_id FROM approvals
             WHERE scope = 'run' AND run_id = ?1 ORDER BY granted_at DESC, id DESC",
            &[SqlValue::from(run)],
        )?
        .iter()
        .map(approval_of)
        .collect()
}

/// What an approval row holds besides its identifier.
pub(crate) struct NewApproval {
    pub(crate) id: String,
    pub(crate) workspace: String,
    pub(crate) run: String,
    pub(crate) diff_hash: String,
    pub(crate) tool_version: String,
    pub(crate) approver: String,
    pub(crate) channel: &'static str,
    pub(crate) presence: &'static str,
    pub(crate) expires_at: Timestamp,
}

pub(crate) fn insert_approval(
    tx: &mut Tx<'_>,
    key: &PolicyKey,
    new: NewApproval,
) -> Result<(), Failure> {
    let now = tx.now();
    tables(tx, key)?.execute(
        "INSERT INTO approvals
            (id, workspace_id, scope, run_id, diff_hash, tool_version_hash, approver_id, channel,
             presence_check, granted_at, expires_at)
         VALUES (?1, ?2, 'run', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        &[
            SqlValue::from(new.id),
            SqlValue::from(new.workspace),
            SqlValue::from(new.run),
            SqlValue::from(new.diff_hash),
            SqlValue::from(new.tool_version),
            SqlValue::from(new.approver),
            SqlValue::from(new.channel),
            SqlValue::from(new.presence),
            SqlValue::from(now),
            SqlValue::from(new.expires_at),
        ],
    )?;
    Ok(())
}

/// Marks an approval used, if it is still unused. Gives whether it did.
pub(crate) fn mark_used(tx: &mut Tx<'_>, key: &PolicyKey, id: &str) -> Result<bool, Failure> {
    let now = tx.now();
    let changed = tables(tx, key)?.execute(
        "UPDATE approvals SET used_at = ?2 WHERE id = ?1 AND used_at IS NULL AND revoked_at IS NULL",
        &[SqlValue::from(id), SqlValue::from(now)],
    )?;
    Ok(changed == 1)
}

pub(crate) fn insert_decline(
    tx: &mut Tx<'_>,
    key: &PolicyKey,
    id: String,
    workspace: String,
    run: String,
    by: Option<String>,
    channel: &'static str,
) -> Result<(), Failure> {
    let now = tx.now();
    tables(tx, key)?.execute(
        "INSERT INTO approval_declines (id, workspace_id, run_id, declined_by, channel, declined_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        &[
            SqlValue::from(id),
            SqlValue::from(workspace),
            SqlValue::from(run),
            SqlValue::from(by),
            SqlValue::from(channel),
            SqlValue::from(now),
        ],
    )?;
    Ok(())
}

/// Revokes every unused approval of a run. Gives how many it revoked.
pub(crate) fn revoke_unused(tx: &mut Tx<'_>, key: &PolicyKey, run: &str) -> Result<u64, Failure> {
    let now = tx.now();
    tables(tx, key)?.execute(
        "UPDATE approvals SET revoked_at = ?2
         WHERE scope = 'run' AND run_id = ?1 AND used_at IS NULL AND revoked_at IS NULL",
        &[SqlValue::from(run), SqlValue::from(now)],
    )
}

/// Whether a run's diff was declined.
pub(crate) fn declined(tx: &mut Tx<'_>, key: &PolicyKey, run: &str) -> Result<bool, Failure> {
    Ok(tables(tx, key)?
        .query_first(
            "SELECT 1 FROM approval_declines WHERE run_id = ?1 LIMIT 1",
            &[SqlValue::from(run)],
        )?
        .is_some())
}
