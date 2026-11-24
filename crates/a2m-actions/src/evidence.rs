//! What the publish review says this engine proved (decision 0311).
//!
//! No claim about evidence is written into a file. `fixture.record` writes
//! an entry in this engine's ledger (its audit chain) with the digest of the
//! fixture's folder, the run it came from, and the address it was recorded
//! against; a `binding.promote` writes one with the digest of the binding it
//! promoted. The review looks each fixture's current digest and each
//! verified binding up there, so a clone, or a machine of CI, shows that the
//! engine recorded nothing, which is true, and editing a file cannot change
//! the answer.

use a2m_core::{Digest, EvidenceLevel, Failure, OperationName, Plan, TargetName, ToolRuns};
use a2m_ledger::AuditRow;
use serde_json::{Value as Json, json};

use crate::kernel::Kernel;
use crate::offline::GateRun;

/// The action a recorded fixture is written under in the ledger.
pub(crate) const FIXTURE_RECORD: &str = "fixture.record";

/// The action a promoted binding is written under in the ledger.
pub(crate) const BINDING_PROMOTE: &str = "binding.promote";

/// How many entries of the chain are read at once.
const PAGE: u32 = 500;

/// The detail of the ledger entry of a recorded fixture.
pub(crate) fn fixture_detail(
    target: &TargetName,
    folder: &str,
    digest: Digest,
    run: &str,
    address: &str,
) -> String {
    json!({
        "target": target.as_str(),
        "fixture": folder,
        "digest": digest.to_string(),
        "run": run,
        "address": address,
    })
    .to_string()
}

/// The mark a `binding.promote` entry carries at the end of its detail, so
/// that the review can tell which binding it proved.
pub(crate) fn promoted_mark(digest: Digest) -> String {
    format!("[binding {digest}]")
}

/// What the ledger holds of one fixture folder at the digest it has now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Fixture {
    /// This engine recorded exactly these bytes.
    Recorded {
        /// The run the fixture came from.
        run: String,
        /// The address it was recorded against.
        address: String,
    },
    /// The ledger holds no recording of these bytes.
    NotRecorded,
}

/// Every entry of the audit chain, oldest first.
pub(crate) async fn chain(kernel: &Kernel) -> Result<Vec<AuditRow>, Failure> {
    let (Ok(store), Some(installation)) = (kernel.store(), kernel.inner.installation) else {
        return Ok(Vec::new());
    };
    let workspace = installation.workspace;
    store
        .read(move |tx| {
            let mut all = Vec::new();
            let mut after = 0;
            loop {
                let page = tx.audit().entries(&workspace, after, PAGE)?;
                let Some(last) = page.last() else {
                    return Ok(all);
                };
                after = last.seq;
                let more = page.len() == PAGE as usize;
                all.extend(page);
                if !more {
                    return Ok(all);
                }
            }
        })
        .await
}

/// What the entries say of the fixture folder `folder` of `target`, whose
/// bytes now have the digest `current`.
pub(crate) fn fixture_of(
    rows: &[AuditRow],
    target: &TargetName,
    folder: &str,
    current: Option<Digest>,
) -> Fixture {
    let Some(current) = current else {
        return Fixture::NotRecorded;
    };
    rows.iter()
        .rev()
        .filter(|row| row.action == FIXTURE_RECORD)
        .filter_map(|row| serde_json::from_str::<Json>(&row.detail).ok())
        .find(|detail| {
            detail["target"] == target.as_str()
                && detail["fixture"] == folder
                && detail["digest"] == current.to_string()
        })
        .map_or(Fixture::NotRecorded, |detail| Fixture::Recorded {
            run: detail["run"].as_str().unwrap_or_default().to_owned(),
            address: detail["address"].as_str().unwrap_or_default().to_owned(),
        })
}

/// Whether a `binding.promote` of this ledger proved the binding of
/// `operation` of `target` that does what `digest` names.
pub(crate) fn promoted(
    rows: &[AuditRow],
    target: &TargetName,
    operation: &OperationName,
    digest: Digest,
) -> bool {
    let subject = format!("{target}/{operation}");
    let mark = promoted_mark(digest);
    rows.iter().any(|row| {
        row.action == BINDING_PROMOTE && row.subject == subject && row.detail.ends_with(&mark)
    })
}

/// A tool's level at review, by the meaning of the levels (decision 0311,
/// point 3): with only written fixtures, at most `reconstructed`; with a
/// fixture this engine recorded and that is unchanged, `schema_verified`;
/// with a trial this engine's ledger holds, `executed`.
pub(crate) fn level(has_trial: bool, has_recorded_fixture: bool) -> EvidenceLevel {
    if has_trial {
        EvidenceLevel::Executed
    } else if has_recorded_fixture {
        EvidenceLevel::SchemaVerified
    } else {
        EvidenceLevel::Reconstructed
    }
}

/// The address a trial of `operation` was made against: that of the surface
/// of the binding it ran, in the target's default environment.
pub(crate) fn address_of(
    plan: &Plan,
    operation: &a2m_core::OperationPlan,
    pin: Option<&a2m_engine::BindingPin>,
) -> String {
    let position = pin.map_or(0, |pin| pin.position);
    let Some(binding) = operation
        .bindings
        .get(position)
        .or_else(|| operation.bindings.first())
    else {
        return String::new();
    };
    let Some(surface) = plan.surface(binding.surface) else {
        return String::new();
    };
    plan.default_environment()
        .and_then(|environment| {
            environment
                .addresses
                .iter()
                .find(|over| over.surface == binding.surface)
        })
        .map_or_else(|| surface.address.clone(), |over| over.address.clone())
}

/// One line of the review for each fact the ledger gives of this version's
/// evidence, the bindings marked `verified` that no promote proved first,
/// then each tool below `executed`, then the others, then each fixture.
pub(crate) async fn lines(
    kernel: &Kernel,
    caller: &crate::dispatch::Caller,
    target: &TargetName,
    plan: &Plan,
    gates: &GateRun,
) -> Result<Vec<String>, Failure> {
    let rows = chain(kernel).await?;
    let mut lines = Vec::new();

    for operation in &plan.operations {
        for (position, binding) in operation.bindings.iter().enumerate() {
            if binding.status != a2m_core::BindingStatus::Verified {
                continue;
            }
            let proved = a2m_engine::binding_digest(plan, binding)
                .is_some_and(|digest| promoted(&rows, target, &operation.name, digest));
            if !proved {
                lines.push(format!(
                    "Marked verified by hand: {} ({}), the binding {} through the mode {} says `verified`, and no `binding.promote` in this engine's ledger proved it.",
                    operation.name,
                    operation.effect,
                    position + 1,
                    binding.mode
                ));
            }
        }
    }

    let fixtures: Vec<(String, Fixture)> = gates
        .fixtures
        .iter()
        .map(|(folder, digest)| (folder.clone(), fixture_of(&rows, target, folder, *digest)))
        .collect();
    let trials = tools_with_a_trial(kernel, caller, target, plan).await?;
    let tools: Vec<ToolEvidence> = plan
        .tools
        .iter()
        .enumerate()
        .map(|(at, tool)| ToolEvidence {
            name: tool.name.to_string(),
            trial: trials.iter().any(|name| name == &tool.name),
            folders: plan
                .tests
                .iter()
                .filter(|test| test.tool.get() == at)
                .filter_map(|test| test.fixture.clone())
                .collect(),
        })
        .collect();
    lines.extend(tool_and_fixture_lines(&tools, &fixtures));
    Ok(lines)
}

/// What the ledger gives of one tool: whether it holds a trial of it, and
/// the fixture folders of the tool's offline tests.
pub(crate) struct ToolEvidence {
    pub(crate) name: String,
    pub(crate) trial: bool,
    pub(crate) folders: Vec<String>,
}

/// The lines for each tool, those below `executed` first, and for each
/// fixture.
pub(crate) fn tool_and_fixture_lines(
    tools: &[ToolEvidence],
    fixtures: &[(String, Fixture)],
) -> Vec<String> {
    let mut found: Vec<(EvidenceLevel, &str, &str)> = tools
        .iter()
        .map(|tool| {
            let recorded = tool.folders.iter().any(|folder| {
                fixtures.iter().any(|(held, state)| {
                    held == folder && matches!(state, Fixture::Recorded { .. })
                })
            });
            let level = level(tool.trial, recorded);
            let why = match level {
                EvidenceLevel::Executed => "a trial of it is in this engine's ledger",
                EvidenceLevel::SchemaVerified => {
                    "a fixture this engine recorded, unchanged, is a test of it"
                }
                _ => "no trial of it and no fixture this engine recorded is in its ledger",
            };
            (level, tool.name.as_str(), why)
        })
        .collect();
    found.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(right.1)));
    let mut lines: Vec<String> = found
        .iter()
        .map(|(level, name, why)| format!("Evidence of the tool `{name}`: {level}, because {why}."))
        .collect();
    for (folder, state) in fixtures {
        lines.push(match state {
            Fixture::Recorded { run, address } => format!(
                "Fixture `{folder}`: recorded here, from run {run} against address {address}, unchanged."
            ),
            Fixture::NotRecorded => format!("Fixture `{folder}`: not recorded by this engine."),
        });
    }
    lines
}

/// The tools of the plan that a completed trial of the caller's is held for
/// in the ledger: a run that carries the binding it tried.
async fn tools_with_a_trial(
    kernel: &Kernel,
    caller: &crate::dispatch::Caller,
    target: &TargetName,
    plan: &Plan,
) -> Result<Vec<OperationName>, Failure> {
    let Some(person) = caller.principal.person() else {
        return Ok(Vec::new());
    };
    let workspace = caller.principal.workspace;
    let names: Vec<OperationName> = plan
        .tools
        .iter()
        .filter(|tool| matches!(tool.runs, ToolRuns::Operation(_) | ToolRuns::Pipeline(_)))
        .map(|tool| tool.name.clone())
        .collect();
    let target = target.clone();
    kernel
        .store()?
        .read(move |tx| {
            let mut held = Vec::new();
            for name in names {
                let query = a2m_ledger::RunQuery {
                    workspace,
                    principal: person,
                    states: vec![a2m_core::RunState::Completed],
                    tool: Some(name.clone()),
                    target: Some(target.clone()),
                    since: None,
                    caller_key: None,
                    after: None,
                    limit: 20,
                };
                let rows = tx.ledger().runs(&query)?;
                let mut tried = false;
                for row in &rows {
                    if let Some(stored) = crate::runner::run_call(tx, row)?
                        && stored.caller.binding.is_some()
                    {
                        tried = true;
                        break;
                    }
                }
                if tried {
                    held.push(name);
                }
            }
            Ok(held)
        })
        .await
}

/// The ledger entry `fixture.record` writes.
pub(crate) async fn record_fixture(
    kernel: &Kernel,
    actor: &str,
    target: &TargetName,
    folder: &str,
    detail: &str,
) -> Result<(), Failure> {
    kernel
        .audit(actor, FIXTURE_RECORD, &format!("{target}/{folder}"), detail)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(action: &str, subject: &str, detail: String) -> AuditRow {
        AuditRow {
            seq: 1,
            at: a2m_core::Timestamp::from_unix_millis(0),
            actor: "person".to_owned(),
            action: action.to_owned(),
            subject: subject.to_owned(),
            detail,
            previous: Digest::of(b"before"),
            hash: Digest::of(b"this"),
        }
    }

    #[test]
    fn a_fixture_is_recorded_only_at_the_digest_the_ledger_holds() {
        let target = TargetName::new("example").unwrap();
        let held = Digest::of(b"the folder as recorded");
        let rows = [row(
            FIXTURE_RECORD,
            "example/fixtures/one",
            fixture_detail(&target, "fixtures/one", held, "r_1", "http://127.0.0.1:1"),
        )];
        assert_eq!(
            fixture_of(&rows, &target, "fixtures/one", Some(held)),
            Fixture::Recorded {
                run: "r_1".to_owned(),
                address: "http://127.0.0.1:1".to_owned()
            }
        );
        // One byte edited: another digest, so not recorded.
        let edited = Digest::of(b"the folder as recorded, edited");
        assert_eq!(
            fixture_of(&rows, &target, "fixtures/one", Some(edited)),
            Fixture::NotRecorded
        );
        // Another folder, another target, or a folder that was not read.
        assert_eq!(
            fixture_of(&rows, &target, "fixtures/two", Some(held)),
            Fixture::NotRecorded
        );
        let other = TargetName::new("other").unwrap();
        assert_eq!(
            fixture_of(&rows, &other, "fixtures/one", Some(held)),
            Fixture::NotRecorded
        );
        assert_eq!(
            fixture_of(&rows, &target, "fixtures/one", None),
            Fixture::NotRecorded
        );
    }

    #[test]
    fn a_binding_is_proved_only_by_a_promote_of_its_own_digest() {
        let target = TargetName::new("example").unwrap();
        let tool = OperationName::new("example_order_get").unwrap();
        let digest = Digest::of(b"what the binding does");
        let rows = [row(
            BINDING_PROMOTE,
            "example/example_order_get",
            format!(
                "example example_order_get web: draft to verified {}",
                promoted_mark(digest)
            ),
        )];
        assert!(promoted(&rows, &target, &tool, digest));
        assert!(!promoted(
            &rows,
            &target,
            &tool,
            Digest::of(b"another binding")
        ));
        let elsewhere = OperationName::new("example_order_list").unwrap();
        assert!(!promoted(&rows, &target, &elsewhere, digest));
    }

    #[test]
    fn a_version_whose_only_fixtures_were_written_by_hand_is_shown_reconstructed() {
        let tools = [
            ToolEvidence {
                name: "example_a_get".to_owned(),
                trial: false,
                folders: vec!["fixtures/a".to_owned()],
            },
            ToolEvidence {
                name: "example_b_get".to_owned(),
                trial: true,
                folders: vec![],
            },
        ];
        let fixtures = [("fixtures/a".to_owned(), Fixture::NotRecorded)];
        let lines = tool_and_fixture_lines(&tools, &fixtures);
        // The tool below `executed` is named first, at `reconstructed`.
        assert!(
            lines[0].starts_with("Evidence of the tool `example_a_get`: reconstructed"),
            "{lines:?}"
        );
        assert!(lines[1].contains("`example_b_get`: executed"), "{lines:?}");
        assert_eq!(
            lines[2],
            "Fixture `fixtures/a`: not recorded by this engine."
        );
        // The same tool with a fixture this engine recorded: schema_verified.
        let recorded = [(
            "fixtures/a".to_owned(),
            Fixture::Recorded {
                run: "r_1".to_owned(),
                address: "http://x".to_owned(),
            },
        )];
        let lines = tool_and_fixture_lines(&tools, &recorded);
        assert!(lines[0].contains("schema_verified"), "{lines:?}");
        assert!(
            lines[2].ends_with("recorded here, from run r_1 against address http://x, unchanged."),
            "{lines:?}"
        );
    }

    #[test]
    fn a_level_follows_what_the_ledger_holds() {
        assert_eq!(level(false, false), EvidenceLevel::Reconstructed);
        assert_eq!(level(false, true), EvidenceLevel::SchemaVerified);
        assert_eq!(level(true, false), EvidenceLevel::Executed);
        assert_eq!(level(true, true), EvidenceLevel::Executed);
    }
}
