//! The drift marks a canary or a run leaves, in `actions_drift_marks`
//! (decision 0380): kept when the engine reports a binding that drifted,
//! lifted when a canary's pass vouches for the same knowledge, and read by
//! the runner into a job's conditions, so that every binding the drifted
//! knowledge stops is refused before anything is sent.

use a2m_core::{
    CompiledTarget, Failure, FailureCode, Mode, OperationName, RunId, SurfaceName, WorkspaceId,
};
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

/// One mark of a target, as it was kept: what it names, the entry's name
/// (empty for a mark of a whole binding), and when it was made.
pub(crate) struct Held {
    pub mark: DriftMark,
    pub entry: String,
    pub found_at: i64,
}

/// The marks of a target, or that one of them cannot be read: what it said
/// is not known, and unknown means stop.
pub(crate) enum Marks {
    Read(Vec<Held>),
    Unreadable,
}

/// The marks kept of the target of `compiled_target`.
pub(crate) fn marks(
    tx: &mut Tx<'_>,
    workspace: WorkspaceId,
    target: &str,
) -> Result<Marks, Failure> {
    let rows = tx.tables(Owner::Actions).query(
        "SELECT surface, mode, knowledge, digest, entry, operation, found_at
         FROM actions_drift_marks WHERE workspace_id = ?1 AND target = ?2
         ORDER BY found_at",
        &[
            SqlValue::from(workspace.to_string()),
            SqlValue::from(target),
        ],
    )?;
    let mut marks = Vec::with_capacity(rows.len());
    for row in rows {
        let read = || -> Option<Held> {
            let digest = row.text(3).ok()?.parse().ok()?;
            let entry = row.text(4).ok()?.to_owned();
            Some(Held {
                mark: DriftMark {
                    surface: SurfaceName::new(row.text(0).ok()?).ok()?,
                    mode: Mode::new(row.text(1).ok()?).ok()?,
                    knowledge: match row.text(2).ok()? {
                        "entry" => DriftKnowledge::Entry {
                            section: digest,
                            entry: entry.clone(),
                        },
                        "binding" => DriftKnowledge::Binding(digest),
                        _ => return None,
                    },
                    operation: OperationName::new(row.text(5).ok()?).ok()?,
                },
                entry,
                found_at: row.int(6).ok()?,
            })
        };
        match read() {
            Some(held) => marks.push(held),
            None => return Ok(Marks::Unreadable),
        }
    }
    Ok(Marks::Read(marks))
}

/// Every binding of `compiled_target`, by its operation and mode.
fn every_binding(compiled_target: &CompiledTarget) -> Vec<(OperationName, Mode)> {
    compiled_target
        .operations
        .iter()
        .flat_map(|operation| {
            operation
                .bindings
                .iter()
                .map(|binding| (operation.name.clone(), binding.mode.clone()))
        })
        .collect()
}

/// Every binding of `compiled_target`, by its operation and mode, that a mark
/// of its target stops. A mark that cannot be read stops every binding of the
/// compiled target: what it said is not known, and unknown means stop.
pub(crate) fn stopped(
    tx: &mut Tx<'_>,
    workspace: WorkspaceId,
    compiled_target: &CompiledTarget,
) -> Result<Vec<(OperationName, Mode)>, Failure> {
    match marks(tx, workspace, compiled_target.header.target.as_str())? {
        Marks::Unreadable => Ok(every_binding(compiled_target)),
        Marks::Read(held) => {
            let marks: Vec<DriftMark> = held.into_iter().map(|held| held.mark).collect();
            Ok(a2m_engine::drift_covers(compiled_target, &marks))
        }
    }
}

/// Why a tool is unavailable now (decision 0396): the knowledge a mark that
/// stops it names, as the mark holds it, and when the mark was made. The
/// name is the entry's, or, for a mark of a whole binding, the operation's
/// whose binding drifted. A mark that cannot be read names nothing and
/// says no time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Unavailable {
    pub knowledge: String,
    pub marked_at: Option<i64>,
}

impl Unavailable {
    /// As `tool.search` and `tool.describe` give it.
    pub(crate) fn json(&self) -> serde_json::Value {
        let mut out = serde_json::json!({"reason": "drift", "knowledge": self.knowledge});
        if let (Some(at), serde_json::Value::Object(map)) = (self.marked_at, &mut out) {
            map.insert("marked_at".to_owned(), serde_json::json!(at));
        }
        out
    }
}

/// Which tools of `compiled_target` the marks of its target make unavailable
/// now (decision 0396): an operation every binding of which a mark stops,
/// as the engine would refuse it; a pipeline with a step whose operation is
/// one. Each names the latest mark that stops it.
pub(crate) fn unavailable(
    compiled_target: &CompiledTarget,
    marks: &Marks,
) -> std::collections::BTreeMap<OperationName, Unavailable> {
    // Each binding, with what stops it.
    let mut stops: std::collections::BTreeMap<(OperationName, Mode), Unavailable> =
        std::collections::BTreeMap::new();
    match marks {
        Marks::Unreadable => {
            for pair in every_binding(compiled_target) {
                stops.insert(
                    pair,
                    Unavailable {
                        knowledge: String::new(),
                        marked_at: None,
                    },
                );
            }
        }
        Marks::Read(held) => {
            for one in held {
                let named = if one.entry.is_empty() {
                    one.mark.operation.as_str().to_owned()
                } else {
                    one.entry.clone()
                };
                // Kept in the order they were made, so the latest wins.
                for pair in
                    a2m_engine::drift_covers(compiled_target, std::slice::from_ref(&one.mark))
                {
                    stops.insert(
                        pair,
                        Unavailable {
                            knowledge: named.clone(),
                            marked_at: Some(one.found_at),
                        },
                    );
                }
            }
        }
    }
    let mut out = std::collections::BTreeMap::new();
    if stops.is_empty() {
        return out;
    }
    let latest = |found: Option<Unavailable>, next: &Unavailable| match found {
        Some(found) if found.marked_at >= next.marked_at => Some(found),
        _ => Some(next.clone()),
    };
    for operation in &compiled_target.operations {
        if operation.bindings.is_empty() {
            continue;
        }
        let mut why = None;
        let every = operation.bindings.iter().all(|binding| {
            match stops.get(&(operation.name.clone(), binding.mode.clone())) {
                Some(stop) => {
                    why = latest(why.take(), stop);
                    true
                }
                None => false,
            }
        });
        if let (true, Some(why)) = (every, why) {
            out.insert(operation.name.clone(), why);
        }
    }
    for pipeline in &compiled_target.pipelines {
        let mut why = None;
        for step in &pipeline.steps {
            if let Some(stop) = compiled_target
                .operation(step.operation)
                .and_then(|operation| out.get(&operation.name))
            {
                why = latest(why.take(), stop);
            }
        }
        if let Some(why) = why {
            out.insert(pipeline.name.clone(), why);
        }
    }
    out
}
