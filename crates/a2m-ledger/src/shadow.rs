//! A binding in shadow: how it compared with the binding that did the real
//! work of a live call, whether a person may promote it, and the record of
//! the promotion (spec section 201, decision 0207).
//!
//! Every comparison is one row, never changed. What a binding has earned is
//! counted from the rows when it is asked for, by the binding's digest
//! (decision 0200): a binding whose steps change has another digest and has
//! earned nothing. A mismatch, in any environment and against any verified
//! binding, starts the count again; a shadow that was not compared neither
//! counts nor starts it again. The ledger never promotes by itself: it
//! answers whether a person may, and records the promotion a person makes,
//! checking the answer again in the same transaction.
//!
//! The evidence says what it covers (decision 0251). Each comparison names
//! the call's input by the workspace's keyed digest of it, which is never
//! shown; the environment, from the run; and the verified binding it was
//! compared against. What makes a binding promotable is the number of
//! **distinct inputs** that matched since the last mismatch, against the
//! verified binding that will act beside it, in each environment the
//! evidence must come from: five calls with one input are one input.

use std::collections::BTreeMap;

use a2m_core::{
    Digest, DigestPurpose, EffectClass, Environment, Failure, FailureClass, FailureCode, Mode,
    OperationName, PrincipalId, RunId, RunState, TargetName, Timestamp, Values, WorkspaceId,
};
use serde_json::{Value as Json, json};

use crate::codec::{moment, parse, small};
use crate::failure;
use crate::ledger::{EventKind, Fence};
use crate::tx::{Row, SqlValue, Tx};

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
    /// The inputs of the call it was made beside. The ledger keeps only
    /// the workspace's keyed digest of them, made in the transaction; the
    /// inputs are never stored, and the digest is never shown.
    pub input: Values,
    /// What the shadow read cost the system, in the rate budget's units:
    /// one per step it was granted. Zero for a shadow that sent nothing.
    pub requests: u32,
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
    /// The digest of the verified binding that acts beside it once the
    /// promotion is published: only matches against it count (decision
    /// 0251).
    pub against: Digest,
    /// The environments the evidence must come from. Each must hold the
    /// threshold of distinct inputs on its own; matches in any other count
    /// for nothing. Never empty.
    pub environments: Vec<Environment>,
}

/// How many distinct inputs that matched a binding needs, in each
/// environment its evidence must come from, before a person may promote it:
/// the settings `engine.shadow.matches_to_promote.read` and `.write` (spec
/// section 226, decision 0251).
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
    /// The environment it was compared in.
    pub environment: Environment,
    /// The outputs that differed, and how.
    pub differing: Vec<DifferingOutput>,
}

/// What one environment the evidence must come from holds toward a
/// promotion: counted since the last mismatch, against the verified binding
/// named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentEvidence {
    /// The environment.
    pub environment: Environment,
    /// The distinct inputs that matched. The threshold applies to this.
    pub distinct_inputs: u32,
    /// The matches among them: an input that matched twice is two matches
    /// and one distinct input.
    pub matches: u32,
}

/// What a binding holds in one environment it was compared in: what
/// `shadow.report` shows for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentStanding {
    /// The environment.
    pub environment: Environment,
    /// Shadow reads kept in it, of every outcome.
    pub reads: u32,
    /// What those reads cost the system, in the rate budget's units.
    pub requests: u64,
    /// Matches since the last mismatch, against any verified binding.
    pub matches_since: u32,
    /// Of those, the matches against the verified binding the standing
    /// counts against.
    pub against_counted: u32,
    /// The distinct inputs among those.
    pub distinct_inputs: u32,
    /// The last comparison among those.
    pub through_seq: Option<u64>,
}

impl EnvironmentStanding {
    fn evidence(&self) -> EnvironmentEvidence {
        EnvironmentEvidence {
            environment: self.environment.clone(),
            distinct_inputs: self.distinct_inputs,
            matches: self.against_counted,
        }
    }
}

/// A promotion a person made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromotionRow {
    /// The operation.
    pub operation: OperationName,
    /// The binding's digest: what it was promoted as.
    pub binding: Digest,
    /// Its mode.
    pub mode: Mode,
    /// The person.
    pub promoted_by: PrincipalId,
    /// When.
    pub promoted_at: Timestamp,
    /// The fewest distinct inputs that matched in any environment its
    /// evidence came from. For a promotion made before decision 0251, the
    /// exact matches in a row.
    pub matches: u32,
    /// The threshold in force.
    pub threshold: u32,
    /// The last comparison counted.
    pub through_seq: u64,
    /// The verified binding the matches were compared against. `None` for
    /// a promotion made before decision 0251.
    pub against: Option<Digest>,
    /// Each environment its evidence came from, and what it held. Empty
    /// for a promotion made before decision 0251.
    pub evidence: Vec<EnvironmentEvidence>,
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
    /// Exact matches since the last mismatch, or since the first
    /// comparison, in every environment and against every verified
    /// binding. Not what makes it promotable: see `environments`.
    pub in_a_row: u32,
    /// The last comparison of those matches, when there is one.
    pub in_a_row_through: Option<u64>,
    /// The verified binding the counts of `environments` are against: the
    /// one asked about, or the one its newest comparison was against.
    pub against: Digest,
    /// What its shadow reads cost the system, in all, in the rate budget's
    /// units.
    pub requests: u64,
    /// Each environment it was compared in, in name order.
    pub environments: Vec<EnvironmentStanding>,
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
    /// Fewer distinct inputs matched than the threshold in at least one
    /// environment the evidence must come from, and no mismatch.
    TooFew {
        /// Each environment the evidence must come from, in name order.
        evidence: Vec<EnvironmentEvidence>,
        /// The threshold.
        threshold: u32,
    },
    /// It mismatched, and has not matched the threshold's number of
    /// distinct inputs since, in every environment its evidence must come
    /// from.
    MismatchSince {
        /// Each environment the evidence must come from, counted since the
        /// mismatch.
        evidence: Vec<EnvironmentEvidence>,
        /// The threshold.
        threshold: u32,
        /// What differed at the last mismatch.
        differing: Vec<DifferingOutput>,
    },
    /// A person promoted it already.
    Promoted(PromotionRow),
}

/// The words of a list of evidence: "3 distinct inputs in `test`".
fn evidence_words(evidence: &[EnvironmentEvidence]) -> String {
    evidence
        .iter()
        .map(|held| {
            format!(
                "{} distinct input(s) in `{}`",
                held.distinct_inputs, held.environment
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

impl NotPromotable {
    fn sentence(&self) -> String {
        match self {
            Self::WritesNotBuilt => "The operation writes, and the shadow of a write is not built: such a binding is not promoted through shadow.".to_owned(),
            Self::NothingCompared => "Nothing was ever compared for the binding as it is now: it is new, or what it does changed since it was compared.".to_owned(),
            Self::TooFew { evidence, threshold } => format!(
                "The binding matched the verified one on {}, and {threshold} distinct inputs are needed in each.",
                evidence_words(evidence)
            ),
            Self::MismatchSince { evidence, threshold, differing } => format!(
                "The binding differed from the verified one in {} output(s) at its last mismatch, and has matched it since on {}; {threshold} distinct inputs are needed in each.",
                differing.len(),
                evidence_words(evidence)
            ),
            Self::Promoted(_) => "A person promoted the binding already.".to_owned(),
        }
    }
}

/// The answer to whether a binding may be promoted now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Promotable {
    /// It may: in every environment its evidence must come from, at least
    /// the threshold of distinct inputs matched the verified binding named,
    /// since the last mismatch, through this comparison.
    Yes {
        /// Each environment the evidence came from, in name order.
        evidence: Vec<EnvironmentEvidence>,
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
    crate::codes::row::SHADOW_NOT_COUNTED.failure(
        format!("The shadow comparison is not counted: {what}."),
        "Nothing to do; a shadow is counted only beside a live read under way.",
    )
}

fn write_not_built() -> Failure {
    crate::codes::row::SHADOW_WRITE_NOT_BUILT.failure(
        "The shadow of a binding of an operation that writes is not built.",
        "Keep the operation on its verified binding; a binding that writes is not promoted through shadow yet.",
    )
}

/// The purpose a shadow comparison's input is digested under: its own
/// (decision 0262). A comparison kept before migration 0023 was digested
/// under `caller_key`; its row says so in `input_purpose`, and only rows of
/// this purpose count as distinct inputs.
const INPUT_PURPOSE: DigestPurpose = DigestPurpose::ShadowInput;

/// What the input's digest is made over: a label, the operation, and the
/// input's canonical bytes.
fn input_bytes(operation: &OperationName, input: &Values) -> Vec<u8> {
    let mut bytes = b"anything2mcp shadow comparison input 2".to_vec();
    bytes.push(0);
    bytes.extend_from_slice(operation.as_str().as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&input.canonical_bytes());
    bytes
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
            let name = item["name"].as_str().ok_or_else(|| {
                failure::corrupt("a stored shadow comparison that no longer reads")
            })?;
            let how = item["how"].as_str().ok_or_else(|| {
                failure::corrupt("a stored shadow comparison that no longer reads")
            })?;
            Ok(DifferingOutput {
                name: name.to_owned(),
                how: Difference::parse(how)?,
            })
        })
        .collect()
}

fn evidence_json(evidence: &[EnvironmentEvidence]) -> Result<String, Failure> {
    let list: Vec<Json> = evidence
        .iter()
        .map(|held| {
            json!({
                "environment": held.environment.as_str(),
                "distinct_inputs": held.distinct_inputs,
                "matches": held.matches,
            })
        })
        .collect();
    serde_json::to_string(&list).map_err(|_| failure::invalid("a promotion's evidence"))
}

fn read_evidence(text: Option<&str>) -> Result<Vec<EnvironmentEvidence>, Failure> {
    let Some(text) = text else {
        return Ok(Vec::new());
    };
    let corrupt = || failure::corrupt("a stored promotion's evidence that no longer reads");
    let parsed: Json = serde_json::from_str(text).map_err(|_| corrupt())?;
    let items = parsed.as_array().ok_or_else(corrupt)?;
    items
        .iter()
        .map(|item| {
            let number = |field: &str| {
                item[field]
                    .as_u64()
                    .and_then(|number| u32::try_from(number).ok())
                    .ok_or_else(corrupt)
            };
            Ok(EnvironmentEvidence {
                environment: parse(item["environment"].as_str().ok_or_else(corrupt)?)?,
                distinct_inputs: number("distinct_inputs")?,
                matches: number("matches")?,
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
        against: crate::codec::parse_opt(row.opt_text(8)?)?,
        evidence: read_evidence(row.opt_text(9)?)?,
    })
}

/// The evidence of each environment asked for, in name order: what the
/// standing holds for it, or nothing.
fn required(standing: &ShadowStanding, environments: &[Environment]) -> Vec<EnvironmentEvidence> {
    let mut asked: Vec<&Environment> = environments.iter().collect();
    asked.sort();
    asked.dedup();
    asked
        .into_iter()
        .map(|environment| {
            standing
                .environments
                .iter()
                .find(|held| held.environment == *environment)
                .map_or_else(
                    || EnvironmentEvidence {
                        environment: environment.clone(),
                        distinct_inputs: 0,
                        matches: 0,
                    },
                    EnvironmentStanding::evidence,
                )
        })
        .collect()
}

impl Shadows<'_, '_> {
    /// Keeps one comparison, and answers its sequence number. Workspace,
    /// target, and environment are taken from the run, never from the
    /// caller; the input is kept as the workspace's keyed digest only. In
    /// the same transaction, the run gets an event that says a shadow read
    /// was made beside it, how it compared, and what it cost in requests.
    ///
    /// # Errors
    ///
    /// `busy`/`shadow_write_not_built` for an operation that writes.
    /// `policy`/`shadow_not_counted` in an offline test's own store, beside
    /// a run that is not `running`, or beside a run that writes.
    /// `possible_commit`/`runner_lost` when the fence is not the run's
    /// current lease. `input`/`schema_invalid` for a mismatch that names no
    /// output. `input`/`not_found` for a run the store does not hold.
    /// `busy`/`store_unavailable` before the vault is attached.
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
        let input = self
            .tx
            .shared
            .attached()?
            .digest
            .keyed_digest(
                &run.workspace,
                INPUT_PURPOSE,
                &input_bytes(&comparison.operation, &comparison.input),
            )
            .to_string();
        let at = self.tx.now();
        self.tx.exec(
            "INSERT INTO shadow_comparisons
                (workspace_id, target, environment, operation, binding_digest, mode, position,
                 against_digest, run_id, writes, outcome, differing, why_class, why_code, at,
                 input_digest, requests, input_purpose)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
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
                input.into(),
                comparison.requests.into(),
                INPUT_PURPOSE.as_str().into(),
            ],
        )?;
        let row = self
            .tx
            .row("SELECT last_insert_rowid()", &[])?
            .ok_or_else(|| failure::corrupt("a row that gave no number"))?;
        let seq = seq_of(row.int(0)?)?;
        // The run says that a second read of the system was made beside
        // its call, and what it cost (decision 0251): names and counts,
        // never a value or a digest.
        let said = json!({
            "shadow_read": true,
            "operation": comparison.operation.as_str(),
            "mode": comparison.mode.as_str(),
            "position": comparison.position,
            "outcome": comparison.outcome.word(),
            "requests": comparison.requests,
            "comparison": seq,
        });
        self.tx
            .ledger()
            .append_event(&comparison.fence, EventKind::Need, &said, None)?;
        Ok(seq)
    }

    /// What one binding, by its digest, has earned, with its counts toward
    /// promotion against `against`, or, when that is `None`, against the
    /// verified binding its newest comparison was against. `None` when
    /// nothing was ever compared for it.
    ///
    /// # Errors
    ///
    /// `busy` failures of the database; `busy`/`store_corrupt` for a row
    /// that no longer reads.
    #[allow(
        clippy::too_many_lines,
        reason = "one standing, read query by query; each part is a few lines"
    )]
    pub fn standing(
        &mut self,
        workspace: &WorkspaceId,
        target: &TargetName,
        operation: &OperationName,
        binding: &Digest,
        against: Option<&Digest>,
    ) -> Result<Option<ShadowStanding>, Failure> {
        let key = || -> Vec<SqlValue> {
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
                _ => {
                    return Err(failure::corrupt(
                        "a stored shadow outcome that no longer reads",
                    ));
                }
            }
            first_at = first_at.min(row.int(2)?);
            last_at = last_at.max(row.int(3)?);
        }
        // A mismatch anywhere, against any verified binding, starts the
        // count again: evidence against a binding is never narrowed.
        let last = self.tx.row(
            "SELECT seq, at, run_id, differing, environment FROM shadow_comparisons
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
                    environment: parse(row.text(4)?)?,
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
        let newest = self
            .tx
            .row(
                "SELECT mode, against_digest FROM shadow_comparisons
                 WHERE workspace_id = ?1 AND target = ?2 AND operation = ?3 AND binding_digest = ?4
                 ORDER BY seq DESC LIMIT 1",
                &key(),
            )?
            .ok_or_else(|| failure::corrupt("a count that gave no row"))?;
        let mode = parse(newest.text(0)?)?;
        let against: Digest = match against {
            Some(against) => *against,
            None => parse(newest.text(1)?)?,
        };
        let mut by_environment = key();
        by_environment.push(since.into());
        by_environment.push(against.to_string().into());
        // Only inputs digested under the purpose in force are counted: one
        // input digested under two purposes would be counted twice, and an
        // older comparison counts as no distinct input (decision 0262).
        by_environment.push(INPUT_PURPOSE.as_str().into());
        let rows = self.tx.rows(
            "SELECT environment,
                    COUNT(*),
                    COALESCE(SUM(requests), 0),
                    COALESCE(SUM(outcome = 'match' AND seq > ?5), 0),
                    COALESCE(SUM(outcome = 'match' AND seq > ?5 AND against_digest = ?6), 0),
                    COUNT(DISTINCT CASE WHEN outcome = 'match' AND seq > ?5
                                             AND against_digest = ?6 AND input_purpose = ?7
                                        THEN input_digest END),
                    MAX(CASE WHEN outcome = 'match' AND seq > ?5 AND against_digest = ?6
                             AND input_purpose = ?7 AND input_digest IS NOT NULL THEN seq END)
             FROM shadow_comparisons
             WHERE workspace_id = ?1 AND target = ?2 AND operation = ?3 AND binding_digest = ?4
             GROUP BY environment
             ORDER BY environment",
            &by_environment,
        )?;
        let mut environments = Vec::with_capacity(rows.len());
        let mut requests = 0_u64;
        for row in &rows {
            let spent = crate::codec::count(row.int(2)?)?;
            requests = requests.saturating_add(spent);
            environments.push(EnvironmentStanding {
                environment: parse(row.text(0)?)?,
                reads: small(row.int(1)?)?,
                requests: spent,
                matches_since: small(row.int(3)?)?,
                against_counted: small(row.int(4)?)?,
                distinct_inputs: small(row.int(5)?)?,
                through_seq: row.opt_int(6)?.map(seq_of).transpose()?,
            });
        }
        let promoted = self
            .tx
            .row(
                "SELECT operation, binding_digest, mode, promoted_by, promoted_at, matches,
                        threshold, through_seq, against_digest, evidence
                 FROM shadow_promotions
                 WHERE workspace_id = ?1 AND target = ?2 AND operation = ?3
                   AND binding_digest = ?4",
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
            against,
            requests,
            environments,
            last_mismatch,
            first_at: moment(first_at),
            last_at: moment(last_at),
            promoted,
        }))
    }

    /// Whether a person may promote the binding now, and why not: at least
    /// the threshold of distinct inputs must have matched the subject's
    /// verified binding since the last mismatch, in every environment the
    /// subject names.
    ///
    /// # Errors
    ///
    /// `input`/`schema_invalid` for a subject that names no environment;
    /// as [`Shadows::standing`].
    pub fn promotable(
        &mut self,
        subject: &ShadowSubject,
        threshold: ShadowThreshold,
    ) -> Result<Promotable, Failure> {
        if subject.writes {
            return Ok(Promotable::No(NotPromotable::WritesNotBuilt));
        }
        if subject.environments.is_empty() {
            return Err(failure::invalid(
                "a promotion that names no environment its evidence must come from",
            ));
        }
        let threshold = threshold.for_operation(subject.writes);
        let Some(standing) = self.standing(
            &subject.workspace,
            &subject.target,
            &subject.operation,
            &subject.binding,
            Some(&subject.against),
        )?
        else {
            return Ok(Promotable::No(NotPromotable::NothingCompared));
        };
        if let Some(promoted) = standing.promoted {
            return Ok(Promotable::No(NotPromotable::Promoted(promoted)));
        }
        let evidence = required(&standing, &subject.environments);
        let through_seq = standing
            .environments
            .iter()
            .filter(|held| subject.environments.contains(&held.environment))
            .filter_map(|held| held.through_seq)
            .max();
        if evidence
            .iter()
            .all(|held| held.distinct_inputs >= threshold)
            && let Some(through_seq) = through_seq
        {
            return Ok(Promotable::Yes {
                evidence,
                threshold,
                through_seq,
            });
        }
        if standing.matches == 0 && standing.mismatches == 0 {
            return Ok(Promotable::No(NotPromotable::NothingCompared));
        }
        Ok(Promotable::No(match standing.last_mismatch {
            Some(last) => NotPromotable::MismatchSince {
                evidence,
                threshold,
                differing: last.differing,
            },
            None => NotPromotable::TooFew {
                evidence,
                threshold,
            },
        }))
    }

    /// Records a person's promotion of a binding, on the evidence counted
    /// in this very transaction: the binding's digest it was promoted as,
    /// the verified binding its matches were against, and each
    /// environment's distinct inputs. It does not change the binding's
    /// status: that is in the target's files, which the caller rewrites,
    /// after checking that the binding still has the digest the row names.
    ///
    /// # Errors
    ///
    /// `policy`/`shadow_not_promotable` when [`Shadows::promotable`] says
    /// no, with the reason in the message; as [`Shadows::promotable`].
    pub fn promote(&mut self, promotion: &ShadowPromotion) -> Result<PromotionRow, Failure> {
        let subject = &promotion.subject;
        let (evidence, threshold, through_seq) = match self
            .promotable(subject, promotion.threshold)?
        {
            Promotable::Yes {
                evidence,
                threshold,
                through_seq,
            } => (evidence, threshold, through_seq),
            Promotable::No(why) => {
                return Err(crate::codes::row::SHADOW_NOT_PROMOTABLE.failure(
                    why.sentence(),
                    "See `anything2mcp shadow report` for how the binding compared, and promote it when it has earned it.",
                ));
            }
        };
        let fewest = evidence
            .iter()
            .map(|held| held.distinct_inputs)
            .min()
            .unwrap_or(0);
        let row = PromotionRow {
            operation: subject.operation.clone(),
            binding: subject.binding,
            mode: promotion.mode.clone(),
            promoted_by: promotion.promoted_by,
            promoted_at: self.tx.now(),
            matches: fewest,
            threshold,
            through_seq,
            against: Some(subject.against),
            evidence,
        };
        self.tx.exec(
            "INSERT INTO shadow_promotions
                (workspace_id, target, operation, binding_digest, mode, promoted_by, promoted_at,
                 matches, threshold, through_seq, against_digest, evidence)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
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
                subject.against.to_string().into(),
                evidence_json(&row.evidence)?.into(),
            ],
        )?;
        Ok(row)
    }

    /// What every binding of the target compared in shadow has earned, by
    /// operation and digest, in that order: the data of `shadow.report`.
    /// `operation` keeps the report to one operation. `against` names, by
    /// operation, the verified binding the counts toward promotion are
    /// against; an operation it does not name is counted against the
    /// verified binding its newest comparison was against.
    ///
    /// # Errors
    ///
    /// As [`Shadows::standing`].
    pub fn report(
        &mut self,
        workspace: &WorkspaceId,
        target: &TargetName,
        operation: Option<&OperationName>,
        against: &BTreeMap<OperationName, Digest>,
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
            if let Some(standing) = self.standing(
                workspace,
                target,
                &operation,
                &binding,
                against.get(&operation),
            )? {
                report.push(standing);
            }
        }
        Ok(report)
    }
}
