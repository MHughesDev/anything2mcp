//! Findings and the unknowns ledger, as stored data (spec section 218).
//!
//! A finding is named by the digest of its content and is never rewritten. A
//! finding is `inferred` or `discovered` and nothing higher: the type in
//! `a2m-core` refuses to build or to read back anything else, and the table
//! refuses the row.

use a2m_core::{Digest, Failure, Finding, RunId, TargetName, Timestamp, WorkspaceId};

use crate::blob::{BlobRef, words};
use crate::codec::{moment, parse};
use crate::failure;
use crate::tx::Tx;

/// The format findings are stored in now. Findings in an older format are
/// kept and are not rewritten; they are re-observed.
pub const FINDING_FORMAT: u32 = 1;

/// The identifier of one open question, shown with the prefix `u_`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UnknownId([u8; 16]);

impl UnknownId {
    /// What the identifier starts with when written.
    pub const PREFIX: &'static str = "u";
}

impl std::fmt::Display for UnknownId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}_", Self::PREFIX)?;
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl std::fmt::Debug for UnknownId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "UnknownId({self})")
    }
}

impl std::str::FromStr for UnknownId {
    type Err = Failure;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        // The same shape as every other identifier, under its own prefix.
        let rest = text
            .strip_prefix("u_")
            .ok_or_else(|| failure::invalid("an identifier of an open question"))?;
        let as_run: RunId = format!("{}_{rest}", RunId::PREFIX)
            .parse()
            .map_err(|_| failure::invalid("an identifier of an open question"))?;
        Ok(Self(*as_run.as_bytes()))
    }
}

words! {
    /// Where an open question stands (spec section 218).
    UnknownStatus {
        /// Asked, and nobody is looking.
        Open => "open",
        /// Being looked at.
        Looking => "looking",
        /// Cannot be looked at until something else happens.
        Blocked => "blocked",
        /// Settled by evidence.
        Settled => "settled",
        /// Decided not to matter.
        OutOfScope => "out_of_scope",
    }
}

/// An open question about a target.
#[derive(Debug, Clone)]
pub struct NewUnknown {
    /// The workspace.
    pub workspace: WorkspaceId,
    /// The target the question is about.
    pub target: TargetName,
    /// The question.
    pub question: String,
    /// What it is asked of: a screen, an operation, a surface.
    pub scope: String,
    /// Which draft or tool depends on the answer.
    pub why_it_matters: String,
    /// The kind of evidence that would settle it, and at what level.
    pub settled_by_what: String,
}

/// What settled a question: findings, gate evidence, or both. A question is
/// settled by evidence, never by a statement, so one of the two is required.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settlement {
    /// The stored findings that answer it.
    pub findings: Vec<Digest>,
    /// The stored gate evidence that answers it.
    pub evidence: Vec<BlobRef>,
}

/// An open question, as the store holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownRow {
    /// Its identifier.
    pub id: UnknownId,
    /// The target.
    pub target: TargetName,
    /// The question.
    pub question: String,
    /// What it is asked of.
    pub scope: String,
    /// Which draft or tool depends on the answer.
    pub why_it_matters: String,
    /// What would settle it.
    pub settled_by_what: String,
    /// Where it stands.
    pub status: UnknownStatus,
    /// What settled it, when it is settled.
    pub settlement: Option<Settlement>,
    /// When it was asked.
    pub opened_at: Timestamp,
}

/// The findings store and the unknowns ledger, inside one transaction.
pub struct Findings<'t, 'a> {
    tx: &'t mut Tx<'a>,
}

impl<'a> Tx<'a> {
    /// The findings store and the unknowns ledger.
    pub fn findings(&mut self) -> Findings<'_, 'a> {
        Findings { tx: self }
    }
}

fn read_finding(body: &str) -> Result<Finding, Failure> {
    // Reading checks the finding again: its level against its source, and
    // its identifier against its content.
    serde_json::from_str(body)
        .map_err(|_| failure::corrupt("a stored finding that no longer holds"))
}

impl Findings<'_, '_> {
    /// Stores a finding about `target`. Storing the same finding again
    /// changes nothing: it is the same observation.
    ///
    /// # Errors
    ///
    /// `busy` failures of the database.
    pub fn put(
        &mut self,
        workspace: &WorkspaceId,
        target: &TargetName,
        finding: &Finding,
    ) -> Result<(), Failure> {
        let body = serde_json::to_string(finding)
            .map_err(|_| failure::invalid("a finding that cannot be written"))?;
        self.tx.exec(
            "INSERT INTO findings
                (workspace_id, target, id, format, level, kind, source, body, observed_at, stored_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT (workspace_id, target, id) DO NOTHING",
            &[
                workspace.to_string().into(),
                target.as_str().into(),
                finding.id().to_string().into(),
                FINDING_FORMAT.into(),
                finding.level().as_str().into(),
                finding.kind().as_str().into(),
                finding.source().as_str().into(),
                body.into(),
                finding.observed_at().into(),
                self.tx.now().into(),
            ],
        )?;
        Ok(())
    }

    /// The finding with this identifier, or `None`.
    ///
    /// # Errors
    ///
    /// `busy`/`store_corrupt` when the stored finding no longer holds: a
    /// level above what its source allows, or an identifier that is not the
    /// digest of its content.
    pub fn get(
        &mut self,
        workspace: &WorkspaceId,
        target: &TargetName,
        id: &Digest,
    ) -> Result<Option<Finding>, Failure> {
        self.tx
            .row(
                "SELECT body FROM findings
                 WHERE workspace_id = ?1 AND target = ?2 AND id = ?3 AND format = ?4",
                &[
                    workspace.to_string().into(),
                    target.as_str().into(),
                    id.to_string().into(),
                    FINDING_FORMAT.into(),
                ],
            )?
            .map(|row| read_finding(row.text(0)?))
            .transpose()
    }

    /// Every finding about `target` in the current format, in the order they
    /// were observed.
    ///
    /// # Errors
    ///
    /// As [`Findings::get`].
    pub fn list(
        &mut self,
        workspace: &WorkspaceId,
        target: &TargetName,
    ) -> Result<Vec<Finding>, Failure> {
        self.tx
            .rows(
                "SELECT body FROM findings
                 WHERE workspace_id = ?1 AND target = ?2 AND format = ?3
                 ORDER BY observed_at, id",
                &[
                    workspace.to_string().into(),
                    target.as_str().into(),
                    FINDING_FORMAT.into(),
                ],
            )?
            .iter()
            .map(|row| read_finding(row.text(0)?))
            .collect()
    }

    /// Records an open question. Its text passes the redactor.
    ///
    /// # Errors
    ///
    /// `busy` failures of the database and of the random source.
    pub fn open_question(&mut self, new: NewUnknown) -> Result<UnknownId, Failure> {
        let random = self.tx.random()?;
        let id = UnknownId(*RunId::from_parts(self.tx.now(), random)?.as_bytes());
        let redact = std::sync::Arc::clone(&self.tx.shared.attached()?.redact);
        self.tx.exec(
            "INSERT INTO unknowns
                (id, workspace_id, target, question, scope, why_it_matters, settled_by_what,
                 status, opened_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'open', ?8, ?8)",
            &[
                id.to_string().into(),
                new.workspace.to_string().into(),
                new.target.as_str().into(),
                redact.redact(&new.question).into(),
                redact.redact(&new.scope).into(),
                redact.redact(&new.why_it_matters).into(),
                redact.redact(&new.settled_by_what).into(),
                self.tx.now().into(),
            ],
        )?;
        Ok(id)
    }

    /// Changes where a question stands, to anything but `settled`.
    ///
    /// # Errors
    ///
    /// `unknown_variant`/`ledger_state_unexpected` for `settled`, which only
    /// [`Findings::settle`] gives, and for a question already settled.
    /// `input`/`not_found`.
    pub fn set_status(&mut self, id: UnknownId, status: UnknownStatus) -> Result<(), Failure> {
        if status == UnknownStatus::Settled {
            return Err(failure::unexpected(
                "a question is settled by evidence, with `settle`",
            ));
        }
        let changed = self.tx.exec(
            "UPDATE unknowns SET status = ?2, updated_at = ?3 WHERE id = ?1 AND status <> 'settled'",
            &[id.to_string().into(), status.as_str().into(), self.tx.now().into()],
        )?;
        if changed == 1 {
            return Ok(());
        }
        match self.unknown(id)? {
            Some(_) => Err(failure::unexpected("a settled question stays settled")),
            None => Err(failure::not_found("open question")),
        }
    }

    /// Settles a question with evidence. Each finding named must be stored
    /// for the question's target, and each piece of evidence must be a
    /// stored blob.
    ///
    /// # Errors
    ///
    /// `unknown_variant`/`ledger_state_unexpected` when no evidence is
    /// given, when a finding or a blob named is not stored, or when the
    /// question is settled already. `input`/`not_found`.
    pub fn settle(&mut self, id: UnknownId, settlement: &Settlement) -> Result<(), Failure> {
        if settlement.findings.is_empty() && settlement.evidence.is_empty() {
            return Err(failure::unexpected(
                "a question is settled by evidence, never by a statement",
            ));
        }
        let row = self
            .tx
            .row(
                "SELECT workspace_id, target, status FROM unknowns WHERE id = ?1",
                &[id.to_string().into()],
            )?
            .ok_or_else(|| failure::not_found("open question"))?;
        if row.text(2)? == UnknownStatus::Settled.as_str() {
            return Err(failure::unexpected("a settled question stays settled"));
        }
        for finding in &settlement.findings {
            let stored = self.tx.row(
                "SELECT 1 FROM findings WHERE workspace_id = ?1 AND target = ?2 AND id = ?3",
                &[
                    row.text(0)?.into(),
                    row.text(1)?.into(),
                    finding.to_string().into(),
                ],
            )?;
            if stored.is_none() {
                return Err(failure::unexpected(
                    "a finding named as settlement is not stored",
                ));
            }
        }
        for blob in &settlement.evidence {
            if self
                .tx
                .row("SELECT 1 FROM blobs WHERE hash = ?1", &[blob.into()])?
                .is_none()
            {
                return Err(failure::unexpected(
                    "evidence named as settlement is not stored",
                ));
            }
        }
        let text = serde_json::json!({
            "findings": settlement.findings.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "evidence": settlement.evidence.iter().map(ToString::to_string).collect::<Vec<_>>(),
        })
        .to_string();
        self.tx.exec(
            "UPDATE unknowns SET status = 'settled', settlement = ?2, updated_at = ?3 WHERE id = ?1",
            &[id.to_string().into(), text.into(), self.tx.now().into()],
        )?;
        Ok(())
    }

    /// One question, or `None`.
    ///
    /// # Errors
    ///
    /// `busy` failures of the database.
    pub fn unknown(&mut self, id: UnknownId) -> Result<Option<UnknownRow>, Failure> {
        self.tx
            .row(
                "SELECT id, target, question, scope, why_it_matters, settled_by_what, status,
                        settlement, opened_at
                 FROM unknowns WHERE id = ?1",
                &[id.to_string().into()],
            )?
            .map(|row| unknown_row(&row))
            .transpose()
    }

    /// The questions about `target`, oldest first.
    ///
    /// # Errors
    ///
    /// `busy` failures of the database.
    pub fn unknowns(
        &mut self,
        workspace: &WorkspaceId,
        target: &TargetName,
    ) -> Result<Vec<UnknownRow>, Failure> {
        self.tx
            .rows(
                "SELECT id, target, question, scope, why_it_matters, settled_by_what, status,
                        settlement, opened_at
                 FROM unknowns WHERE workspace_id = ?1 AND target = ?2 ORDER BY id",
                &[workspace.to_string().into(), target.as_str().into()],
            )?
            .iter()
            .map(unknown_row)
            .collect()
    }
}

fn unknown_row(row: &crate::tx::Row) -> Result<UnknownRow, Failure> {
    let unreadable = || failure::corrupt("a stored settlement that no longer reads");
    let settlement = row
        .opt_text(7)?
        .map(|text| -> Result<Settlement, Failure> {
            let json: serde_json::Value = serde_json::from_str(text).map_err(|_| unreadable())?;
            let list = |name: &str| -> Result<Vec<Digest>, Failure> {
                json.get(name)
                    .and_then(serde_json::Value::as_array)
                    .ok_or_else(unreadable)?
                    .iter()
                    .map(|entry| entry.as_str().ok_or_else(unreadable).and_then(parse))
                    .collect()
            };
            Ok(Settlement {
                findings: list("findings")?,
                evidence: list("evidence")?
                    .into_iter()
                    .map(BlobRef::from_digest)
                    .collect(),
            })
        })
        .transpose()?;
    Ok(UnknownRow {
        id: parse(row.text(0)?)?,
        target: parse(row.text(1)?)?,
        question: row.text(2)?.to_owned(),
        scope: row.text(3)?.to_owned(),
        why_it_matters: row.text(4)?.to_owned(),
        settled_by_what: row.text(5)?.to_owned(),
        status: parse(row.text(6)?)?,
        settlement,
        opened_at: moment(row.int(8)?),
    })
}
