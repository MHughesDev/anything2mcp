//! The drift marks a canary or a run leaves, in `actions_drift_marks`
//! (decision 0380): kept when the engine reports a binding that drifted,
//! lifted when a canary's pass vouches for the same knowledge, and read by
//! the runner into a job's conditions, so that every binding the drifted
//! knowledge stops is refused before anything is sent.

use a2m_core::{Failure, FailureCode, Mode, OperationName, Plan, RunId, SurfaceName, WorkspaceId};
use a2m_engine::{DriftKnowledge, DriftMark};
use a2m_ledger::{Owner, SqlValue, Tx};

/// What one mark names: the knowledge's kind, its digest, and the entry's
/// name, empty for a binding.
fn parts(knowledge: &DriftKnowledge) -> (&'static str, String, &str) {
    match knowledge {
        DriftKnowledge::Entry { section, entry } => ("entry", section.to_string(), entry.as_str()),
        DriftKnowledge::Binding(digest) => ("binding", digest.to_string(), ""),
    }
}

/// One drift a run found, as the control keeps it.
pub(crate) struct Found<'a> {
    pub workspace: WorkspaceId,
    pub target: &'a str,
    pub run: RunId,
    pub operation: &'a OperationName,
    pub mode: &'a Mode,
    pub surface: &'a SurfaceName,
    pub code: &'a FailureCode,
}

/// Keeps a mark of each piece of `knowledge` that drifted; a mark already
/// kept is replaced, so it names the latest run that saw it.
pub(crate) fn keep(
    tx: &mut Tx<'_>,
    found: &Found<'_>,
    knowledge: &[DriftKnowledge],
) -> Result<(), Failure> {
    let now = tx.now().as_unix_millis();
    for known in knowledge {
        let (kind, digest, entry) = parts(known);
        tx.tables(Owner::Actions).execute(
            "INSERT INTO actions_drift_marks
                (workspace_id, target, surface, mode, knowledge, digest, entry,
                 operation, code, run_id, found_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT (workspace_id, target, surface, mode, knowledge, digest, entry)
             DO UPDATE SET operation = excluded.operation, code = excluded.code,
                 run_id = excluded.run_id, found_at = excluded.found_at",
            &[
                SqlValue::from(found.workspace.to_string()),
                SqlValue::from(found.target),
                SqlValue::from(found.surface.as_str()),
                SqlValue::from(found.mode.as_str()),
                SqlValue::from(kind),
                SqlValue::from(digest),
                SqlValue::from(entry),
                SqlValue::from(found.operation.as_str()),
                SqlValue::from(found.code.as_str()),
                SqlValue::from(found.run.to_string()),
                SqlValue::from(now),
            ],
        )?;
    }
    Ok(())
}

/// Lifts the marks of each piece of `knowledge` a canary's pass through
/// `mode` on `surface` vouched for again.
pub(crate) fn lift(
    tx: &mut Tx<'_>,
    workspace: WorkspaceId,
    target: &str,
    surface: &SurfaceName,
    mode: &Mode,
    knowledge: &[DriftKnowledge],
) -> Result<(), Failure> {
    for known in knowledge {
        let (kind, digest, entry) = parts(known);
        tx.tables(Owner::Actions).execute(
            "DELETE FROM actions_drift_marks
             WHERE workspace_id = ?1 AND target = ?2 AND surface = ?3 AND mode = ?4
               AND knowledge = ?5 AND digest = ?6 AND entry = ?7",
            &[
                SqlValue::from(workspace.to_string()),
                SqlValue::from(target),
                SqlValue::from(surface.as_str()),
                SqlValue::from(mode.as_str()),
                SqlValue::from(kind),
                SqlValue::from(digest),
                SqlValue::from(entry),
            ],
        )?;
    }
    Ok(())
}

/// Every binding of `plan`, by its operation and mode, that a mark of its
/// target stops. A mark that cannot be read stops every binding of the
/// plan: what it said is not known, and unknown means stop.
pub(crate) fn stopped(
    tx: &mut Tx<'_>,
    workspace: WorkspaceId,
    plan: &Plan,
) -> Result<Vec<(OperationName, Mode)>, Failure> {
    let rows = tx.tables(Owner::Actions).query(
        "SELECT surface, mode, knowledge, digest, entry, operation
         FROM actions_drift_marks WHERE workspace_id = ?1 AND target = ?2",
        &[
            SqlValue::from(workspace.to_string()),
            SqlValue::from(plan.header.target.as_str()),
        ],
    )?;
    let mut marks = Vec::with_capacity(rows.len());
    for row in rows {
        let read = || -> Option<DriftMark> {
            let digest = row.text(3).ok()?.parse().ok()?;
            Some(DriftMark {
                surface: SurfaceName::new(row.text(0).ok()?).ok()?,
                mode: Mode::new(row.text(1).ok()?).ok()?,
                knowledge: match row.text(2).ok()? {
                    "entry" => DriftKnowledge::Entry {
                        section: digest,
                        entry: row.text(4).ok()?.to_owned(),
                    },
                    "binding" => DriftKnowledge::Binding(digest),
                    _ => return None,
                },
                operation: OperationName::new(row.text(5).ok()?).ok()?,
            })
        };
        match read() {
            Some(mark) => marks.push(mark),
            None => {
                return Ok(plan
                    .operations
                    .iter()
                    .flat_map(|operation| {
                        operation
                            .bindings
                            .iter()
                            .map(|binding| (operation.name.clone(), binding.mode.clone()))
                    })
                    .collect());
            }
        }
    }
    Ok(a2m_engine::drift_covers(plan, &marks))
}
