//! A binding in shadow: how it compared with the binding that did the real
//! work of a live call, whether a person may promote it, and the record of
//! the promotion (spec section 201, decision 0207).
//!
//! Every comparison is one row, never changed. What a binding has earned is
//! counted from the rows when it is asked for, by the binding's digest
//! (decision 0200): a binding whose steps change has another digest and has
//! earned nothing. A mismatch starts the run of matches again; a shadow that
//! was not compared neither counts nor starts it again. The ledger never
//! promotes by itself: it answers whether a person may, and records the
//! promotion a person makes, checking the answer again in the same
//! transaction.

use a2m_core::{
    Digest, EffectClass, Failure, FailureClass, FailureCode, Mode, OperationName, PrincipalId,
    RunId, RunState, TargetName, Timestamp, WorkspaceId,
};
use serde_json::{Value as Json, json};

use crate::codec::{moment, parse, small};
use crate::failure;
use crate::ledger::Fence;
use crate::tx::{Row, Tx};

/// A comparison is not counted: the store is an offline test's, or the run
/// it was made beside is not a live read under way. Class `policy`.
pub const SHADOW_NOT_COUNTED: FailureCode = FailureCode::fixed("shadow_not_counted");
/// A person asked to promote a binding the evidence does not let them
/// promote. Class `policy`.
pub const SHADOW_NOT_PROMOTABLE: FailureCode = FailureCode::fixed("shadow_not_promotable");
/// The shadow of a binding of an operation that writes is not built: its
/// comparison cannot be stored, and it is never promotable through shadow.
/// Class `busy`.
pub const SHADOW_WRITE_NOT_BUILT: FailureCode = FailureCode::fixed("shadow_write_not_built");

/// How one output of the shadow binding differed from the verified one's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Difference {
    /// Both gave a value, and the values are not equal after normalization.
    Differs,
    /// The shadow binding gave no value for it.
    Missing,
    /// The shadow binding gave a value that cannot be read as the output's
    /// type.
    Unreadable,
}

impl Difference {
    /// The word the store keeps.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Differs => "differs",
            Self::Missing => "missing",
            Self::Unreadable => "unreadable",
        }
    }

    fn parse(text: &str) -> Result<Self, Failure> {
        match text {
            "differs" => Ok(Self::Differs),
            "missing" => Ok(Self::Missing),
            "unreadable" => Ok(Self::Unreadable),
            _ => Err(failure::corrupt("a stored difference that no longer reads")),
        }
    }
}

/// One output that differed: its name and how. Never its value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DifferingOutput {
    /// The output's name, as the operation declares it.
    pub name: String,
    /// How it differed.
    pub how: Difference,
}

/// How a shadow binding compared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShadowOutcome {
    /// Every declared output was equal after normalization.
    Matched,
    /// These outputs, in the operation's declared order, were not. Never
    /// empty.
    Mismatched(Vec<DifferingOutput>),
    /// Nothing was compared: the shadow failed with this class and code, or
    /// passed its time bound. Neither a match nor a mismatch.
    NotCompared {
        /// The class of what stopped it.
        class: FailureClass,
        /// Its code.
        code: FailureCode,
    },
}

impl ShadowOutcome {
    fn word(&self) -> &'static str {
        match self {
            Self::Matched => "match",
            Self::Mismatched(_) => "mismatch",
            Self::NotCompared { .. } => "not_compared",
        }
    }
}

/// One comparison to keep: a shadow binding, beside a live call of the run
/// the fence names, under that run's current lease.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShadowComparison {
    /// The run the live call belongs to, and its lease.
    pub fence: Fence,
    /// The operation called.
    pub operation: OperationName,
    /// The shadow binding's digest (decision 0200).
    pub binding: Digest,
    /// Its mode.
    pub mode: Mode,
    /// Its place among the operation's bindings, from 0.
    pub position: u32,
    /// The digest of the binding that did the real work.
    pub against: Digest,
    /// Whether the operation writes. A comparison of one that does is
    /// refused: the shadow of a write is not built.
    pub writes: bool,
    /// How it compared.
    pub outcome: ShadowOutcome,
}

/// A binding as the question of its promotion names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShadowSubject {
    /// The workspace.
    pub workspace: WorkspaceId,
    /// The target.
    pub target: TargetName,
    /// The operation.
    pub operation: OperationName,
    /// The binding's digest as it is now, from the drafts.
    pub binding: Digest,
    /// Whether the operation writes.
    pub writes: bool,
}

/// How many exact matches in a row a binding needs before a person may
/// promote it: the settings `engine.shadow.matches_to_promote.read` and
/// `.write` (spec section 226).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShadowThreshold {
    /// For an operation that reads.
    pub read: u32,
    /// For an operation that writes.
    pub write: u32,
}

impl Default for ShadowThreshold {
    fn default() -> Self {
        Self { read: 5, write: 20 }
    }
}

impl ShadowThreshold {
    /// The threshold for an operation, never below one.
    #[must_use]
    pub fn for_operation(&self, writes: bool) -> u32 {
        let count = if writes { self.write } else { self.read };
        count.max(1)
    }
}

/// The last mismatch of a binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastMismatch {
    /// When it was compared.
    pub at: Timestamp,
    /// The run it was compared beside.
    pub run: RunId,
    /// The outputs that differed, and how.
    pub differing: Vec<DifferingOutput>,
}

/// A promotion a person made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromotionRow {
    /// The operation.
    pub operation: OperationName,
    /// The binding's digest.
    pub binding: Digest,
    /// Its mode.
    pub mode: Mode,
    /// The person.
    pub promoted_by: PrincipalId,
    /// When.
    pub promoted_at: Timestamp,
    /// The exact matches in a row it was promoted on.
    pub matches: u32,
    /// The threshold in force.
    pub threshold: u32,
    /// The last comparison counted.
    pub through_seq: u64,
}

/// What one binding, by its digest, has earned in shadow: what
/// `shadow.report` shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShadowStanding {
    /// The operation.
    pub operation: OperationName,
    /// The binding's digest.
    pub binding: Digest,
    /// Its mode, as its last comparison names it.
    pub mode: Mode,
    /// Exact matches, in all.
    pub matches: u32,
    /// Mismatches, in all.
    pub mismatches: u32,
    /// Shadows that were not compared.
    pub not_compared: u32,
    /// Exact matches since the last mismatch, or since the first comparison.
    pub in_a_row: u32,
    /// The last comparison of the run of matches, when there is one.
    pub in_a_row_through: Option<u64>,
    /// The last mismatch.
    pub last_mismatch: Option<LastMismatch>,
    /// When the first comparison was made.
    pub first_at: Timestamp,
    /// When the last was made.
    pub last_at: Timestamp,
    /// Its promotion, when a person made one.
    pub promoted: Option<PromotionRow>,
}

/// Why a binding may not be promoted now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotPromotable {
    /// The operation writes, and the shadow of a write is not built.
    WritesNotBuilt,
    /// Nothing was ever compared for the binding as it is now: it is new,
    /// or its steps changed since it was compared.
    NothingCompared,
    /// Fewer exact matches in a row than the threshold, and no mismatch.
    TooFew {
        /// Matches in a row.
        in_a_row: u32,
        /// The threshold.
        threshold: u32,
    },
    /// It mismatched, and has not matched the threshold's number of times
    /// in a row since.
    MismatchSince {
        /// Matches in a row since the mismatch.
        in_a_row: u32,
        /// The threshold.
        threshold: u32,
        /// What differed at the last mismatch.
        differing: Vec<DifferingOutput>,
    },
    /// A person promoted it already.
    Promoted(PromotionRow),
}

impl NotPromotable {
    fn sentence(&self) -> String {
        match self {
            Self::WritesNotBuilt => "The operation writes, and the shadow of a write is not built: such a binding is not promoted through shadow.".to_owned(),
            Self::NothingCompared => "Nothing was ever compared for the binding as it is now: it is new, or what it does changed since it was compared.".to_owned(),
            Self::TooFew { in_a_row, threshold } => format!(
                "The binding matched {in_a_row} times in a row, and {threshold} are needed."
            ),
            Self::MismatchSince { in_a_row, threshold, differing } => format!(
                "The binding differed from the verified one in {} output(s) at its last mismatch, and has matched {in_a_row} times in a row since; {threshold} are needed.",
                differing.len()
            ),
            Self::Promoted(_) => "A person promoted the binding already.".to_owned(),
        }
    }
}

/// The answer to whether a binding may be promoted now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Promotable {
    /// It may: it matched this many times in a row, at or above the
    /// threshold, through this comparison.
    Yes {
        /// Matches in a row.
        in_a_row: u32,
        /// The threshold.
        threshold: u32,
        /// The last comparison counted.
        through_seq: u64,
    },
    /// It may not, and why.
    No(NotPromotable),
}

/// A promotion a person asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShadowPromotion {
    /// The binding.
    pub subject: ShadowSubject,
    /// Its mode.
    pub mode: Mode,
    /// The person who promotes it. The caller has checked it is a person:
    /// the ledger cannot tell.
    pub promoted_by: PrincipalId,
    /// The threshold in force.
    pub threshold: ShadowThreshold,
}

/// Shadow comparisons and promotions, inside one transaction.
pub struct Shadows<'t, 'a> {
    tx: &'t mut Tx<'a>,
}

impl<'a> Tx<'a> {
    /// Shadow comparisons and promotions.
    pub fn shadow(&mut self) -> Shadows<'_, 'a> {
        Shadows { tx: self }
    }
}

fn not_counted(what: &str) -> Failure {
    Failure::new(
        FailureClass::Policy,
        SHADOW_NOT_COUNTED,
        format!("The shadow comparison is not counted: {what}."),
        "Nothing to do; a shadow is counted only beside a live read under way.",
    )
}

fn write_not_built() -> Failure {
    Failure::new(
        FailureClass::Busy,
        SHADOW_WRITE_NOT_BUILT,
        "The shadow of a binding of an operation that writes is not built.",
        "Keep the operation on its verified binding; a binding that writes is not promoted through shadow yet.",
    )
}

fn differing_json(differing: &[DifferingOutput]) -> Result<String, Failure> {
    let list: Vec<Json> = differing
        .iter()
        .map(|output| json!({"name": output.name, "how": output.how.as_str()}))
        .collect();
    serde_json::to_string(&list).map_err(|_| failure::invalid("a shadow comparison"))
}

fn read_differing(text: &str) -> Result<Vec<DifferingOutput>, Failure> {
    let parsed: Json = serde_json::from_str(text)
        .map_err(|_| failure::corrupt("a stored shadow comparison that no longer reads"))?;
    let Some(items) = parsed.as_array() else {
        return Err(failure::corrupt(
            "a stored shadow comparison that no longer reads",
        ));
    };
    items
        .iter()
        .map(|item| {
            let name = item["name"]
                .as_str()
                .ok_or_else(|| failure::corrupt("a stored shadow comparison that no longer reads"))?;
            let how = item["how"]
                .as_str()
                .ok_or_else(|| failure::corrupt("a stored shadow comparison that no longer reads"))?;
            Ok(DifferingOutput {
                name: name.to_owned(),
                how: Difference::parse(how)?,
            })
        })
        .collect()
}

fn seq_of(number: i64) -> Result<u64, Failure> {
    u64::try_from(number).map_err(|_| failure::corrupt("a stored sequence number below zero"))
}

fn promotion_row(row: &Row) -> Result<PromotionRow, Failure> {
    Ok(PromotionRow {
        operation: parse(row.text(0)?)?,
        binding: parse(row.text(1)?)?,
        mode: parse(row.text(2)?)?,
        promoted_by: parse(row.text(3)?)?,
        promoted_at: moment(row.int(4)?),
        matches: small(row.int(5)?)?,
        threshold: small(row.int(6)?)?,
        through_seq: seq_of(row.int(7)?)?,
    })
}

impl Shadows<'_, '_> {
    /// Keeps one comparison, and answers its sequence number. Workspace,
    /// target, and environment are taken from the run, never from the
    /// caller.
    ///
    /// # Errors
    ///
    /// `busy`/`shadow_write_not_built` for an operation that writes.
    /// `policy`/`shadow_not_counted` in an offline test's own store, beside
    /// a run that is not `running`, or beside a run that writes.
    /// `possible_commit`/`runner_lost` when the fence is not the run's
    /// current lease. `input`/`schema_invalid` for a mismatch that names no
    /// output. `input`/`not_found` for a run the store does not hold.
    pub fn record(&mut self, comparison: &ShadowComparison) -> Result<u64, Failure> {
        if comparison.writes {
            return Err(write_not_built());
        }
        if self.tx.shared.throwaway {
            return Err(not_counted("an offline test's store counts nothing"));
        }
        let run = self.tx.ledger().run(comparison.fence.run)?;
        if run.epoch != comparison.fence.epoch {
            return Err(failure::runner_lost());
        }
        if run.state != RunState::Running {
            return Err(not_counted("the run it was made beside is not running"));
        }
        if run.effect != EffectClass::Read {
            return Err(not_counted("the run it was made beside writes"));
        }
        let (differing, why_class, why_code) = match &comparison.outcome {
            ShadowOutcome::Matched => (None, None, None),
            ShadowOutcome::Mismatched(differing) => {
                if differing.is_empty() {
                    return Err(failure::invalid("a mismatch that names no output"));
                }
                (Some(differing_json(differing)?), None, None)
            }
            ShadowOutcome::NotCompared { class, code } => (
                None,
                Some(class.as_str().to_owned()),
                Some(code.as_str().to_owned()),
            ),
        };
        let at = self.tx.now();
        self.tx.exec(
            "INSERT INTO shadow_comparisons
                (workspace_id, target, environment, operation, binding_digest, mode, position,
                 against_digest, run_id, writes, outcome, differing, why_class, why_code, at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, ?10, ?11, ?12, ?13, ?14)",
            &[
                run.workspace.to_string().into(),
                run.target.as_str().into(),
                run.environment.as_str().into(),
                comparison.operation.as_str().into(),
                comparison.binding.to_string().into(),
                comparison.mode.as_str().into(),
                comparison.position.into(),
                comparison.against.to_string().into(),
                run.id.to_string().into(),
                comparison.outcome.word().into(),
                differing.into(),
                why_class.into(),
                why_code.into(),
                at.into(),
            ],
        )?;
        let row = self
            .tx
            .row("SELECT last_insert_rowid()", &[])?
            .ok_or_else(|| failure::corrupt("a row that gave no number"))?;
        seq_of(row.int(0)?)
    }

    /// What one binding, by its digest, has earned. `None` when nothing was
    /// ever compared for it.
    ///
    /// # Errors
    ///
    /// `busy` failures of the database; `busy`/`store_corrupt` for a row
    /// that no longer reads.
    pub fn standing(
        &mut self,
        workspace: &WorkspaceId,
        target: &TargetName,
        operation: &OperationName,
        binding: &Digest,
    ) -> Result<Option<ShadowStanding>, Failure> {
        let key = || -> Vec<crate::tx::SqlValue> {
            vec![
                workspace.to_string().into(),
                target.as_str().into(),
                operation.as_str().into(),
                binding.to_string().into(),
            ]
        };
        let totals = self.tx.rows(
            "SELECT outcome, COUNT(*), MIN(at), MAX(at) FROM shadow_comparisons
             WHERE workspace_id = ?1 AND target = ?2 AND operation = ?3 AND binding_digest = ?4
             GROUP BY outcome",
            &key(),
        )?;
        if totals.is_empty() {
            return Ok(None);
        }
        let (mut matches, mut mismatches, mut not_compared) = (0, 0, 0);
        let (mut first_at, mut last_at) = (i64::MAX, i64::MIN);
        for row in &totals {
            let count = small(row.int(1)?)?;
            match row.text(0)? {
                "match" => matches = count,
                "mismatch" => mismatches = count,
                "not_compared" => not_compared = count,
                _ => return Err(failure::corrupt("a stored shadow outcome that no longer reads")),
            }
            first_at = first_at.min(row.int(2)?);
            last_at = last_at.max(row.int(3)?);
        }
        let last = self.tx.row(
            "SELECT seq, at, run_id, differing FROM shadow_comparisons
             WHERE workspace_id = ?1 AND target = ?2 AND operation = ?3 AND binding_digest = ?4
               AND outcome = 'mismatch'
             ORDER BY seq DESC LIMIT 1",
            &key(),
        )?;
        let (since, last_mismatch) = match last {
            Some(row) => (
                row.int(0)?,
                Some(LastMismatch {
                    at: moment(row.int(1)?),
                    run: parse(row.text(2)?)?,
                    differing: read_differing(row.text(3)?)?,
                }),
            ),
            None => (-1, None),
        };
        let mut after = key();
        after.push(since.into());
        let run = self
            .tx
            .row(
                "SELECT COUNT(*), MAX(seq) FROM shadow_comparisons
                 WHERE workspace_id = ?1 AND target = ?2 AND operation = ?3
                   AND binding_digest = ?4 AND outcome = 'match' AND seq > ?5",
                &after,
            )?
            .ok_or_else(|| failure::corrupt("a count that gave no row"))?;
        let in_a_row = small(run.int(0)?)?;
        let in_a_row_through = run.opt_int(1)?.map(seq_of).transpose()?;
        let mode = self
            .tx
            .row(
                "SELECT mode FROM shadow_comparisons
                 WHERE workspace_id = ?1 AND target = ?2 AND operation = ?3 AND binding_digest = ?4
                 ORDER BY seq DESC LIMIT 1",
                &key(),
            )?
            .ok_or_else(|| failure::corrupt("a count that gave no row"))?;
        let mode = parse(mode.text(0)?)?;
        let promoted = self
            .tx
            .row(
                "SELECT operation, binding_digest, mode, promoted_by, promoted_at, matches,
                        threshold, through_seq
                 FROM shadow_promotions
                 WHERE workspace_id = ?1 AND target = ?2 AND operation = ?3 AND binding_digest = ?4",
                &key(),
            )?
            .map(|row| promotion_row(&row))
            .transpose()?;
        Ok(Some(ShadowStanding {
            operation: operation.clone(),
            binding: *binding,
            mode,
            matches,
            mismatches,
            not_compared,
            in_a_row,
            in_a_row_through,
            last_mismatch,
            first_at: moment(first_at),
            last_at: moment(last_at),
            promoted,
        }))
    }

    /// Whether a person may promote the binding now, and why not.
    ///
    /// # Errors
    ///
    /// As [`Shadows::standing`].
    pub fn promotable(
        &mut self,
        subject: &ShadowSubject,
        threshold: ShadowThreshold,
    ) -> Result<Promotable, Failure> {
        if subject.writes {
            return Ok(Promotable::No(NotPromotable::WritesNotBuilt));
        }
        let threshold = threshold.for_operation(subject.writes);
        let Some(standing) = self.standing(
            &subject.workspace,
            &subject.target,
            &subject.operation,
            &subject.binding,
        )?
        else {
            return Ok(Promotable::No(NotPromotable::NothingCompared));
        };
        if let Some(promoted) = standing.promoted {
            return Ok(Promotable::No(NotPromotable::Promoted(promoted)));
        }
        if standing.in_a_row >= threshold
            && let Some(through_seq) = standing.in_a_row_through
        {
            return Ok(Promotable::Yes {
                in_a_row: standing.in_a_row,
                threshold,
                through_seq,
            });
        }
        if standing.matches == 0 && standing.mismatches == 0 {
            return Ok(Promotable::No(NotPromotable::NothingCompared));
        }
        Ok(Promotable::No(match standing.last_mismatch {
            Some(last) => NotPromotable::MismatchSince {
                in_a_row: standing.in_a_row,
                threshold,
                differing: last.differing,
            },
            None => NotPromotable::TooFew {
                in_a_row: standing.in_a_row,
                threshold,
            },
        }))
    }

    /// Records a person's promotion of a binding, on the evidence counted
    /// in this very transaction. It does not change the binding's status:
    /// that is in the target's files, which the caller rewrites.
    ///
    /// # Errors
    ///
    /// `policy`/`shadow_not_promotable` when [`Shadows::promotable`] says
    /// no, with the reason in the message; as [`Shadows::standing`].
    pub fn promote(&mut self, promotion: &ShadowPromotion) -> Result<PromotionRow, Failure> {
        let subject = &promotion.subject;
        let (in_a_row, threshold, through_seq) =
            match self.promotable(subject, promotion.threshold)? {
                Promotable::Yes {
                    in_a_row,
                    threshold,
                    through_seq,
                } => (in_a_row, threshold, through_seq),
                Promotable::No(why) => {
                    return Err(Failure::new(
                        FailureClass::Policy,
                        SHADOW_NOT_PROMOTABLE,
                        why.sentence(),
                        "See `anything2mcp shadow report` for how the binding compared, and promote it when it has earned it.",
                    ));
                }
            };
        let row = PromotionRow {
            operation: subject.operation.clone(),
            binding: subject.binding,
            mode: promotion.mode.clone(),
            promoted_by: promotion.promoted_by,
            promoted_at: self.tx.now(),
            matches: in_a_row,
            threshold,
            through_seq,
        };
        self.tx.exec(
            "INSERT INTO shadow_promotions
                (workspace_id, target, operation, binding_digest, mode, promoted_by, promoted_at,
                 matches, threshold, through_seq)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            &[
                subject.workspace.to_string().into(),
                subject.target.as_str().into(),
                row.operation.as_str().into(),
                row.binding.to_string().into(),
                row.mode.as_str().into(),
                row.promoted_by.to_string().into(),
                row.promoted_at.into(),
                row.matches.into(),
                row.threshold.into(),
                crate::codec::stored(row.through_seq)?.into(),
            ],
        )?;
        Ok(row)
    }

    /// What every binding of the target compared in shadow has earned, by
    /// operation and digest, in that order: the data of `shadow.report`.
    /// `operation` keeps the report to one operation.
    ///
    /// # Errors
    ///
    /// As [`Shadows::standing`].
    pub fn report(
        &mut self,
        workspace: &WorkspaceId,
        target: &TargetName,
        operation: Option<&OperationName>,
    ) -> Result<Vec<ShadowStanding>, Failure> {
        let rows = match operation {
            Some(operation) => self.tx.rows(
                "SELECT DISTINCT operation, binding_digest FROM shadow_comparisons
                 WHERE workspace_id = ?1 AND target = ?2 AND operation = ?3
                 ORDER BY operation, binding_digest",
                &[
                    workspace.to_string().into(),
                    target.as_str().into(),
                    operation.as_str().into(),
                ],
            )?,
            None => self.tx.rows(
                "SELECT DISTINCT operation, binding_digest FROM shadow_comparisons
                 WHERE workspace_id = ?1 AND target = ?2
                 ORDER BY operation, binding_digest",
                &[workspace.to_string().into(), target.as_str().into()],
            )?,
        };
        let mut report = Vec::with_capacity(rows.len());
        for row in rows {
            let operation: OperationName = parse(row.text(0)?)?;
            let binding: Digest = parse(row.text(1)?)?;
            if let Some(standing) = self.standing(workspace, target, &operation, &binding)? {
                report.push(standing);
            }
        }
        Ok(report)
    }
}
