//! A caller's own runs, listed, and a caller's idempotency keys (spec
//! section 208, decision 0160).
//!
//! A key is the caller's own text and may hold anything, so the store keeps
//! the workspace's keyed digest of it and never the key: someone who reads
//! the store and guesses a key cannot confirm the guess.

use a2m_core::{
    Digest, DigestPurpose, Environment, Failure, FailureClass, OperationName, PrincipalId, RunId,
    RunState, TargetName, Timestamp, WorkspaceId,
};

use crate::codec::{moment, parse, parse_opt};
use crate::failure;
use crate::ledger::{Ledger, RunRow, run_row};
use crate::tx::SqlValue;

/// The most runs one listing returns.
pub const RUN_PAGE_MOST: u32 = 200;

/// The longest idempotency key, in characters.
pub const CALLER_KEY_LONGEST: usize = 200;

pub use crate::codes::IDEMPOTENCY_KEY_INVALID;

/// A caller's idempotency key: 1 to [`CALLER_KEY_LONGEST`] characters, each
/// a printable ASCII character other than a space. Its debug print shows
/// no character of it.
#[derive(Clone, PartialEq, Eq)]
pub struct CallerKey(String);

impl std::fmt::Debug for CallerKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CallerKey(..)")
    }
}

impl CallerKey {
    /// Checks a key as a caller gave it.
    ///
    /// # Errors
    ///
    /// `input`/`idempotency_key_invalid`, which quotes nothing of the key.
    pub fn new(text: &str) -> Result<Self, Failure> {
        if text.is_empty()
            || text.len() > CALLER_KEY_LONGEST
            || !text.bytes().all(|byte| byte.is_ascii_graphic())
        {
            return Err(Failure::new(
                FailureClass::Input,
                IDEMPOTENCY_KEY_INVALID,
                format!(
                    "An idempotency key is 1 to {CALLER_KEY_LONGEST} printable characters, with no space."
                ),
                "Send a short key of letters, digits, and marks such as `-`, the same for every repeat of one call.",
            ));
        }
        Ok(Self(text.to_owned()))
    }

    /// The key as the caller gave it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What a listing of a caller's runs asks for. Runs come newest first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunQuery {
    /// The workspace.
    pub workspace: WorkspaceId,
    /// The person whose runs they are.
    pub principal: PrincipalId,
    /// Only runs in these states; every state when empty.
    pub states: Vec<RunState>,
    /// Only runs of this tool.
    pub tool: Option<OperationName>,
    /// Only runs of this target.
    pub target: Option<TargetName>,
    /// Only runs opened at or after this moment, by the store's clock.
    pub since: Option<Timestamp>,
    /// Only the run this key of the same caller is bound to.
    pub caller_key: Option<CallerKey>,
    /// Only runs older than this one: the last run of the previous page.
    pub after: Option<RunId>,
    /// The most runs to return; at most [`RUN_PAGE_MOST`].
    pub limit: u32,
}

/// What the store holds for one caller's key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallerKeyRow {
    /// The digest of the call the key was first given with.
    pub request: Digest,
    /// The run the key is bound to; `None` while the call that holds it
    /// has admitted nothing yet, or stopped before it bound one.
    pub run: Option<RunId>,
    /// When the key was held.
    pub held_at: Timestamp,
}

const SELECT_RUNS: &str =
    "SELECT id, workspace_id, principal_id, client_label, tool_name, tool_kind, target,
        target_version_hash, environment, effect, input_hash, state, state_class,
        state_code, attempt, lease_epoch, diff_hash, diff_ref, approval_id,
        decision_id, result_ref, trace_ref, created_at, started_at, finished_at
 FROM runs
 WHERE workspace_id = ?1 AND principal_id = ?2
   AND (?3 IS NULL OR instr(?3, ',' || state || ',') > 0)
   AND (?4 IS NULL OR tool_name = ?4)
   AND (?5 IS NULL OR target = ?5)
   AND (?6 IS NULL OR created_at >= ?6)
   AND (?7 IS NULL OR id IN (SELECT run_id FROM caller_idempotency
                             WHERE workspace_id = ?1 AND principal_id = ?2 AND key_digest = ?7))
   AND (?8 IS NULL OR created_at < ?8 OR (created_at = ?8 AND rowid < ?9))
 ORDER BY created_at DESC, rowid DESC
 LIMIT ?10";

impl Ledger<'_, '_> {
    /// The keyed digest a key is kept under, made for the purpose of a
    /// caller's key, which no other digest of the workspace uses (decision
    /// 0172).
    fn key_digest(
        &self,
        workspace: &WorkspaceId,
        principal: &PrincipalId,
        key: &CallerKey,
    ) -> Result<String, Failure> {
        let mut bytes = b"anything2mcp caller idempotency key 1".to_vec();
        bytes.push(0);
        bytes.extend_from_slice(principal.as_bytes());
        bytes.extend_from_slice(key.as_str().as_bytes());
        Ok(self
            .tx
            .shared
            .attached()?
            .digest
            .keyed_digest(workspace, DigestPurpose::CallerKey, &bytes)
            .to_string())
    }

    /// A page of one caller's runs, newest first.
    ///
    /// # Errors
    ///
    /// `input`/`not_found` when `after` is no run of this caller; `busy`
    /// failures of the database.
    pub fn runs(&mut self, query: &RunQuery) -> Result<Vec<RunRow>, Failure> {
        let (workspace, principal) = (query.workspace.to_string(), query.principal.to_string());
        let mut after_at = SqlValue::Null;
        let mut after_row = SqlValue::Null;
        if let Some(after) = query.after {
            let row = self
                .tx
                .row(
                    "SELECT created_at, rowid FROM runs
                     WHERE id = ?1 AND workspace_id = ?2 AND principal_id = ?3",
                    &[
                        after.to_string().into(),
                        workspace.clone().into(),
                        principal.clone().into(),
                    ],
                )?
                .ok_or_else(|| failure::not_found("run"))?;
            after_at = row.int(0)?.into();
            after_row = row.int(1)?.into();
        }
        let states = (!query.states.is_empty()).then(|| {
            let mut list = String::from(",");
            for state in &query.states {
                list.push_str(state.as_str());
                list.push(',');
            }
            list
        });
        let key = query
            .caller_key
            .as_ref()
            .map(|key| self.key_digest(&query.workspace, &query.principal, key))
            .transpose()?;
        let rows = self.tx.rows(
            SELECT_RUNS,
            &[
                workspace.into(),
                principal.into(),
                states.into(),
                query.tool.as_ref().map(OperationName::as_str).into(),
                query.target.as_ref().map(TargetName::as_str).into(),
                query.since.into(),
                key.into(),
                after_at,
                after_row,
                query.limit.clamp(1, RUN_PAGE_MOST).into(),
            ],
        )?;
        rows.iter().map(run_row).collect()
    }

    /// What the store holds for a caller's key, if anything.
    ///
    /// # Errors
    ///
    /// `busy` failures of the database.
    pub fn caller_key(
        &mut self,
        workspace: &WorkspaceId,
        principal: &PrincipalId,
        key: &CallerKey,
    ) -> Result<Option<CallerKeyRow>, Failure> {
        let digest = self.key_digest(workspace, principal, key)?;
        let Some(row) = self.tx.row(
            "SELECT request_hash, run_id, held_at FROM caller_idempotency
             WHERE workspace_id = ?1 AND principal_id = ?2 AND key_digest = ?3",
            &[
                workspace.to_string().into(),
                principal.to_string().into(),
                digest.into(),
            ],
        )?
        else {
            return Ok(None);
        };
        Ok(Some(CallerKeyRow {
            request: parse(row.text(0)?)?,
            run: parse_opt(row.opt_text(1)?)?,
            held_at: moment(row.int(2)?),
        }))
    }

    /// Holds a caller's key for a call, before its run is admitted. A key
    /// that is held already is left as it is: the caller reads it with
    /// [`Ledger::caller_key`] first.
    ///
    /// # Errors
    ///
    /// `busy` failures of the database.
    pub fn hold_caller_key(
        &mut self,
        workspace: &WorkspaceId,
        principal: &PrincipalId,
        key: &CallerKey,
        request: &Digest,
    ) -> Result<(), Failure> {
        let digest = self.key_digest(workspace, principal, key)?;
        let now = self.tx.now();
        self.tx.exec(
            "INSERT INTO caller_idempotency
                (workspace_id, principal_id, key_digest, request_hash, run_id, held_at)
             VALUES (?1, ?2, ?3, ?4, NULL, ?5)
             ON CONFLICT DO NOTHING",
            &[
                workspace.to_string().into(),
                principal.to_string().into(),
                digest.into(),
                request.to_string().into(),
                now.into(),
            ],
        )?;
        Ok(())
    }

    /// Binds a held key to the run its call was given. Binding it again to
    /// the same run changes nothing.
    ///
    /// # Errors
    ///
    /// `unknown_variant`/`ledger_state_unexpected` for a key that is not
    /// held, a key bound to another run, or a run of another caller.
    pub fn bind_caller_key(
        &mut self,
        workspace: &WorkspaceId,
        principal: &PrincipalId,
        key: &CallerKey,
        run: RunId,
    ) -> Result<(), Failure> {
        let row = self.run(run)?;
        if row.workspace != *workspace || row.principal != *principal {
            return Err(failure::unexpected(
                "an idempotency key is bound only to a run of its own caller",
            ));
        }
        match self.caller_key(workspace, principal, key)? {
            None => Err(failure::unexpected(
                "an idempotency key is bound only after it is held",
            )),
            Some(held) if held.run == Some(run) => Ok(()),
            Some(held) if held.run.is_some() => Err(failure::unexpected(
                "an idempotency key bound to one run is never bound to another",
            )),
            Some(_) => {
                let digest = self.key_digest(workspace, principal, key)?;
                self.tx.exec(
                    "UPDATE caller_idempotency SET run_id = ?4
                     WHERE workspace_id = ?1 AND principal_id = ?2 AND key_digest = ?3
                       AND run_id IS NULL",
                    &[
                        workspace.to_string().into(),
                        principal.to_string().into(),
                        digest.into(),
                        run.to_string().into(),
                    ],
                )?;
                Ok(())
            }
        }
    }

    /// Lets go of a held key whose call admitted no run, so that the same
    /// call may be made again with it. A bound key is left as it is.
    ///
    /// # Errors
    ///
    /// `busy` failures of the database.
    pub fn release_caller_key(
        &mut self,
        workspace: &WorkspaceId,
        principal: &PrincipalId,
        key: &CallerKey,
    ) -> Result<(), Failure> {
        let digest = self.key_digest(workspace, principal, key)?;
        self.tx.exec(
            "DELETE FROM caller_idempotency
             WHERE workspace_id = ?1 AND principal_id = ?2 AND key_digest = ?3
               AND run_id IS NULL",
            &[
                workspace.to_string().into(),
                principal.to_string().into(),
                digest.into(),
            ],
        )?;
        Ok(())
    }

    /// The newest run a caller opened at or after `since` for this tool,
    /// environment, and input: the run a held key's call was admitted as,
    /// when it stopped before it could bind it.
    ///
    /// # Errors
    ///
    /// `busy` failures of the database.
    pub fn run_called_since(
        &mut self,
        workspace: &WorkspaceId,
        principal: &PrincipalId,
        call: (&OperationName, &Environment, &Digest),
        since: Timestamp,
    ) -> Result<Option<RunId>, Failure> {
        let (tool, environment, input_hash) = call;
        let row = self.tx.row(
            "SELECT id FROM runs
             WHERE workspace_id = ?1 AND principal_id = ?2 AND tool_name = ?3
               AND environment = ?4 AND input_hash = ?5 AND created_at >= ?6
             ORDER BY created_at DESC, rowid DESC LIMIT 1",
            &[
                workspace.to_string().into(),
                principal.to_string().into(),
                tool.as_str().into(),
                environment.as_str().into(),
                input_hash.to_string().into(),
                since.into(),
            ],
        )?;
        row.map(|row| parse(row.text(0)?)).transpose()
    }
}
