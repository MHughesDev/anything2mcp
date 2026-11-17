//! The audit chain: an append-only log of who did what, one chain per
//! workspace, each entry carrying the hash of the one before it.

use a2m_core::{Digest, Failure, Timestamp, WorkspaceId};

use crate::codec::{count, digest_of, moment, parse, stored};
use crate::tx::{Row, Tx};

/// The number of an entry within its workspace's chain, from 1.
pub type AuditSeq = u64;

/// One thing to record.
#[derive(Debug, Clone)]
pub struct AuditEntry {
    /// The workspace whose chain it joins.
    pub workspace: WorkspaceId,
    /// Who did it: a principal's identifier, or the engine.
    pub actor: String,
    /// What was done: the name of an action.
    pub action: String,
    /// What it was done to.
    pub subject: String,
    /// Small JSON with the rest.
    pub detail: String,
}

/// One entry of the chain, as the store holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditRow {
    /// Its number.
    pub seq: AuditSeq,
    /// When it was recorded.
    pub at: Timestamp,
    /// Who did it.
    pub actor: String,
    /// What was done.
    pub action: String,
    /// What it was done to.
    pub subject: String,
    /// The rest.
    pub detail: String,
    /// The hash of the entry before it; of nothing, for the first.
    pub previous: Digest,
    /// The hash of this entry.
    pub hash: Digest,
}

/// What checking a chain found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChainCheck {
    /// Every entry follows the one before it.
    Intact {
        /// How many entries were checked.
        entries: u64,
        /// The hash of the last one, or `None` for an empty chain. Kept
        /// somewhere else, it shows a chain whose end was cut off.
        head: Option<Digest>,
    },
    /// An entry was removed, changed, or put in another place.
    Broken {
        /// The number of the first entry that does not follow.
        at: AuditSeq,
    },
}

/// What the first entry of a chain names as the entry before it.
fn nothing() -> Digest {
    Digest::from_bytes([0; Digest::LENGTH])
}

fn hash_of(workspace: &WorkspaceId, seq: u64, row: &AuditRow) -> Digest {
    digest_of(
        "audit entry 1",
        &[
            ("workspace", workspace.as_bytes()),
            ("seq", &seq.to_le_bytes()),
            ("at", &row.at.as_unix_millis().to_le_bytes()),
            ("actor", row.actor.as_bytes()),
            ("action", row.action.as_bytes()),
            ("subject", row.subject.as_bytes()),
            ("detail", row.detail.as_bytes()),
            ("previous", row.previous.as_bytes()),
        ],
    )
}

fn audit_row(row: &Row) -> Result<AuditRow, Failure> {
    Ok(AuditRow {
        seq: count(row.int(0)?)?,
        at: moment(row.int(1)?),
        actor: row.text(2)?.to_owned(),
        action: row.text(3)?.to_owned(),
        subject: row.text(4)?.to_owned(),
        detail: row.text(5)?.to_owned(),
        previous: parse(row.text(6)?)?,
        hash: parse(row.text(7)?)?,
    })
}

/// The audit chain, inside one transaction.
pub struct Audit<'t, 'a> {
    tx: &'t mut Tx<'a>,
}

impl<'a> Tx<'a> {
    /// The audit chain.
    pub fn audit(&mut self) -> Audit<'_, 'a> {
        Audit { tx: self }
    }
}

impl Audit<'_, '_> {
    /// Appends an entry to its workspace's chain, in the caller's
    /// transaction, so the entry commits with the change it records. Every
    /// text passes the redactor before it is hashed and stored.
    ///
    /// # Errors
    ///
    /// `busy`/`store_unavailable` while no vault is attached; `busy`
    /// failures of the database.
    pub fn append(&mut self, entry: &AuditEntry) -> Result<AuditSeq, Failure> {
        let redact = std::sync::Arc::clone(&self.tx.shared.attached()?.redact);
        let last = self.tx.row(
            "SELECT seq, hash FROM audit WHERE workspace_id = ?1 ORDER BY seq DESC LIMIT 1",
            &[entry.workspace.to_string().into()],
        )?;
        let (seq, previous) = match last {
            Some(row) => (count(row.int(0)?)? + 1, parse(row.text(1)?)?),
            None => (1, nothing()),
        };
        let mut row = AuditRow {
            seq,
            at: self.tx.now(),
            actor: redact.redact(&entry.actor),
            action: redact.redact(&entry.action),
            subject: redact.redact(&entry.subject),
            detail: redact.redact(&entry.detail),
            previous,
            hash: nothing(),
        };
        row.hash = hash_of(&entry.workspace, seq, &row);
        self.tx.exec(
            "INSERT INTO audit
                (workspace_id, seq, at, actor, action, subject, detail, previous_hash, hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            &[
                entry.workspace.to_string().into(),
                stored(seq)?.into(),
                row.at.into(),
                row.actor.into(),
                row.action.into(),
                row.subject.into(),
                row.detail.into(),
                row.previous.to_string().into(),
                row.hash.to_string().into(),
            ],
        )?;
        Ok(seq)
    }

    /// The entries of a chain with a number above `after`, in order, at most
    /// `limit` of them.
    ///
    /// # Errors
    ///
    /// `busy` failures of the database.
    pub fn entries(
        &mut self,
        workspace: &WorkspaceId,
        after: AuditSeq,
        limit: u32,
    ) -> Result<Vec<AuditRow>, Failure> {
        self.tx
            .rows(
                "SELECT seq, at, actor, action, subject, detail, previous_hash, hash FROM audit
                 WHERE workspace_id = ?1 AND seq > ?2 ORDER BY seq LIMIT ?3",
                &[
                    workspace.to_string().into(),
                    stored(after)?.into(),
                    limit.into(),
                ],
            )?
            .iter()
            .map(audit_row)
            .collect()
    }

    /// Checks a workspace's whole chain: that the numbers run from 1 with no
    /// gap, that each entry names the hash of the one before it, and that
    /// each entry's hash is the hash of its content. An entry that was
    /// removed, changed, or put elsewhere breaks one of the three.
    ///
    /// A chain whose last entries were cut off still follows; that is found
    /// by comparing `head` with a copy kept outside the store.
    ///
    /// # Errors
    ///
    /// `busy` failures of the database. A broken chain is not an error of
    /// the check: it is its answer.
    pub fn verify(&mut self, workspace: &WorkspaceId) -> Result<ChainCheck, Failure> {
        let rows = self.tx.rows(
            "SELECT seq, at, actor, action, subject, detail, previous_hash, hash FROM audit
             WHERE workspace_id = ?1 ORDER BY seq",
            &[workspace.to_string().into()],
        )?;
        let mut previous = nothing();
        let mut expected = 1;
        for row in &rows {
            // A row that no longer reads is a changed entry.
            let Ok(entry) = audit_row(row) else {
                return Ok(ChainCheck::Broken { at: expected });
            };
            if entry.seq != expected
                || entry.previous != previous
                || entry.hash != hash_of(workspace, entry.seq, &entry)
            {
                return Ok(ChainCheck::Broken { at: expected });
            }
            previous = entry.hash;
            expected += 1;
        }
        Ok(ChainCheck::Intact {
            entries: expected - 1,
            head: (expected > 1).then_some(previous),
        })
    }
}
