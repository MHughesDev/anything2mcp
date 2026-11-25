//! The repair bundle, and a person's acceptance of the repair it proposes
//! (decision 0999).
//!
//! A bundle is built on asking from what is kept: the latest drift mark of
//! the knowledge named, the published version that holds it, two findings
//! of a discovery of the surface, before and now, and the target's drafts.
//! The kernel reads none of them for meaning: the pack's repair proposer,
//! registered for the findings' kind, is handed the drafts of the file kind
//! the mark's section was compiled from, and says what it would change.
//! The same inputs give the same bundle, and nothing is kept.
//!
//! Only a person accepts, on the engine's page: an agent's call is sent
//! there. Acceptance builds the bundle again, refuses a proposal that is
//! not the one shown, and writes the one draft file through `draft.write`'s
//! own checks. It publishes nothing: the draft goes through the gates and a
//! publish like any other, and only a version that changed the knowledge
//! is not stopped by its mark (decision 0380).

use std::path::Path;

use a2m_core::{CanonicalJson, Digest, Failure, FailureCode, Finding, TargetName, WorkspaceId};
use a2m_files::{CompileMode, LARGEST_TARGET_FILE, TargetRoot};
use a2m_provider::{RepairAsk, RepairFile, RepairProposal};
use serde_json::{Map, Value as Json, json};

use super::build::{compile_in, off_thread, target_folder};
use super::handler;
use crate::codes;
use crate::dispatch::{Call, Caller, Next, Output};
use crate::drift::{self, Held, Marks};
use crate::kernel::Kernel;

/// What a bundle is asked for.
struct Asked {
    target: TargetName,
    knowledge: String,
    before: Option<Digest>,
    now: Digest,
}

fn finding_id(text: &str) -> Result<Digest, Failure> {
    text.parse().map_err(|_| {
        codes::input(
            codes::SCHEMA_INVALID,
            "A finding's identifier is the hexadecimal digest `finding.list` gives.",
            "Pass an identifier from `finding.list`.",
        )
    })
}

fn asked(input: &Map<String, Json>) -> Result<Asked, Failure> {
    let text = |key: &str| input.get(key).and_then(Json::as_str);
    let missing = |key: &str| {
        codes::input(
            codes::SCHEMA_INVALID,
            format!("`{key}` is required."),
            "Give what `repair.propose` takes: the target, the knowledge, and the findings.",
        )
    };
    Ok(Asked {
        target: TargetName::new(text("target").ok_or_else(|| missing("target"))?)?,
        knowledge: text("knowledge")
            .ok_or_else(|| missing("knowledge"))?
            .to_owned(),
        before: text("before").map(finding_id).transpose()?,
        now: finding_id(text("now").ok_or_else(|| missing("now"))?)?,
    })
}

/// A bundle, as it is shown, and the proposal it holds: the file's path,
/// its text before and after, and the proposal's digest.
pub(crate) struct Bundle {
    pub json: Json,
    pub proposal: Option<Proposal>,
}

pub(crate) struct Proposal {
    pub path: String,
    pub text: String,
    pub digest: Digest,
    pub because: Vec<String>,
    pub diff: Vec<(usize, String, String)>,
}

/// The digest that names one proposal: the file, its text before, and its
/// text after.
fn proposal_digest(path: &str, before: &str, after: &str) -> Digest {
    let mut bytes = Vec::with_capacity(path.len() + before.len() + after.len() + 2);
    bytes.extend_from_slice(path.as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(before.as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(after.as_bytes());
    Digest::of(&bytes)
}

/// The lines that differ between two texts, by the line of the text
/// before, from 1: each with the line before and after. Lines added or
/// removed are given against an empty line.
fn line_diff(before: &str, after: &str) -> Vec<(usize, String, String)> {
    let was: Vec<&str> = before.split('\n').collect();
    let is: Vec<&str> = after.split('\n').collect();
    if was.len() == is.len() {
        return was
            .iter()
            .zip(&is)
            .enumerate()
            .filter(|(_, (was, is))| was != is)
            .map(|(at, (was, is))| (at + 1, (*was).to_owned(), (*is).to_owned()))
            .collect();
    }
    // The longest common run of lines, kept; the rest is shown changed.
    let (n, m) = (was.len(), is.len());
    let mut longest = vec![vec![0_u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            longest[i][j] = if was[i] == is[j] {
                longest[i + 1][j + 1] + 1
            } else {
                longest[i + 1][j].max(longest[i][j + 1])
            };
        }
    }
    let (mut i, mut j, mut out) = (0, 0, Vec::new());
    while i < n || j < m {
        if i < n && j < m && was[i] == is[j] {
            i += 1;
            j += 1;
        } else if j < m && (i == n || longest[i][j + 1] >= longest[i + 1][j]) {
            out.push((i + 1, String::new(), is[j].to_owned()));
            j += 1;
        } else {
            out.push((i + 1, was[i].to_owned(), String::new()));
            i += 1;
        }
    }
    out
}

fn shape_json(finding: &Finding) -> Json {
    let found = serde_json::from_str::<Json>(finding.found())
        .unwrap_or_else(|_| Json::from(finding.found()));
    json!({
        "finding": finding.id().to_string(),
        // Every word of it is the system's: the page's own names of its
        // headings, controls, and sections; every value is a type.
        "shape": {"untrusted": found},
    })
}

/// The latest mark of `knowledge` among the marks of a target.
fn latest_mark(marks: Marks, knowledge: &str) -> Result<Held, Failure> {
    let held = match marks {
        Marks::Read(held) => held,
        Marks::Unreadable => {
            return Err(codes::input(
                codes::NOT_FOUND,
                "A drift mark of the target cannot be read, so what drifted is not known.",
                "Publish a version that changes the drifted knowledge, or let a canary pass again.",
            ));
        }
    };
    held.into_iter()
        .filter(|one| !one.entry.is_empty() && one.entry == knowledge)
        .max_by_key(|one| one.found_at)
        .ok_or_else(|| {
            codes::input(
                codes::NOT_FOUND,
                format!("No drift mark of the target names `{}`.", codes::shown(knowledge)),
                "Call `tool.describe` for a tool marked unavailable, and pass its `unavailable.knowledge`.",
            )
        })
}

impl Kernel {
    async fn finding_of(
        &self,
        workspace: WorkspaceId,
        target: &TargetName,
        id: Digest,
    ) -> Result<Finding, Failure> {
        let named = target.clone();
        self.store()?
            .read(move |tx| tx.findings().get(&workspace, &named, &id))
            .await?
            .ok_or_else(|| {
                codes::input(
                    codes::NOT_FOUND,
                    format!("The target `{target}` has no finding with the identifier {id}."),
                    "Call `finding.list` for the identifiers, after a `discover.run` of the surface.",
                )
            })
    }

    /// The repair bundle `input` asks for, built from what is kept.
    pub(crate) async fn repair_bundle(
        &self,
        caller: &Caller,
        project: &Path,
        input: &Map<String, Json>,
    ) -> Result<Bundle, Failure> {
        let asked = asked(input)?;
        let workspace = caller.principal.workspace;
        let target = asked.target.clone();
        let named = target.clone();
        let marks = self
            .store()?
            .read(move |tx| drift::marks(tx, workspace, named.as_str()))
            .await?;
        let held = latest_mark(marks, &asked.knowledge)?;
        let a2m_engine::DriftKnowledge::Entry { section, .. } = &held.mark.knowledge else {
            return Err(codes::input(
                codes::NOT_FOUND,
                "The mark names a whole binding, not knowledge a file holds.",
                "Repair the binding by hand.",
            ));
        };
        let section = *section;
        // The published version that holds the knowledge, and so the file
        // kind its section was compiled from.
        let (compiled_targets, _) = self.current_compiled_targets().await?;
        let compiled_target = compiled_targets
            .into_iter()
            .find(|(name, _, _)| *name == target)
            .map(|(_, _, compiled_target)| compiled_target)
            .ok_or_else(|| {
                codes::input(
                    codes::NOT_FOUND,
                    format!("The target `{target}` has no published version."),
                    "Call `target.list` for the published targets.",
                )
            })?;
        let digest_of = |pack: &a2m_core::PackSection| {
            CanonicalJson::encode(pack)
                .ok()
                .map(|encoded| encoded.digest())
        };
        let kind = compiled_target
            .packs
            .iter()
            .find(|pack| digest_of(pack) == Some(section))
            .map(|pack| pack.kind.clone())
            .ok_or_else(|| {
                codes::input(
                    codes::NOT_FOUND,
                    format!(
                        "The version of `{target}` published now does not hold the knowledge the mark names, so no tool of it is stopped by it."
                    ),
                    "Call `tool.describe` to see which tools are unavailable now.",
                )
            })?;
        let stops: Vec<Json> =
            a2m_engine::drift_covers(&compiled_target, std::slice::from_ref(&held.mark))
                .into_iter()
                .map(|(tool, mode)| json!({"tool": tool.as_str(), "mode": mode.as_str()}))
                .collect();
        let now = self.finding_of(workspace, &target, asked.now).await?;
        let before = match asked.before {
            Some(id) => Some(self.finding_of(workspace, &target, id).await?),
            None => None,
        };
        if let Some(before) = &before {
            if before.kind() != now.kind() {
                return Err(codes::input(
                    codes::SCHEMA_INVALID,
                    "The two findings are of different kinds, so they are not two shapes of one thing.",
                    "Give two findings of the same kind, such as two `screen` findings of one page.",
                ));
            }
            if before.subject() != now.subject() {
                return Err(codes::input(
                    codes::SCHEMA_INVALID,
                    "The two findings are about different places, so they are not two shapes of one thing.",
                    "Give two findings of the same page or place, one from before the change and one from now.",
                ));
            }
        }
        // The drafts of the file kind, and whether they still hold the
        // knowledge that drifted.
        let (kernel, folder) = (self.clone(), project.to_path_buf());
        let compiled = compile_in(
            self,
            folder.clone(),
            &target,
            CompileMode::Draft,
            a2m_files::DRAFT_VERSION,
        )
        .await?
        .ok();
        let drafts_hold_it = compiled.as_ref().is_some_and(|compiled| {
            compiled
                .compiled_target
                .packs
                .iter()
                .any(|pack| digest_of(pack) == Some(section))
        });
        let (named, wanted) = (target.clone(), kind.clone());
        let files: Vec<RepairFile> = off_thread(move || {
            let set = kernel.providers().compiler_set();
            let at = target_folder(&folder, &named)?;
            let sources = a2m_files::load(&TargetRoot::new(at.as_path()), set).map_err(|_| {
                codes::input(
                    codes::SCHEMA_INVALID,
                    format!("The drafts of `{named}` do not read."),
                    "Call `draft.lint` and correct what it names.",
                )
            })?;
            let mut files = Vec::new();
            for file in sources.of_kind(wanted.as_str()) {
                let bytes = at
                    .join(file.path.as_str())?
                    .read(LARGEST_TARGET_FILE as u64)?;
                files.push(RepairFile {
                    path: file.path.as_str().to_owned(),
                    text: String::from_utf8_lossy(&bytes).into_owned(),
                });
            }
            Ok(files)
        })
        .await?;
        let code = FailureCode::new(&held.code)
            .unwrap_or_else(|_| a2m_engine::codes::BINDING_DRIFTED.clone());
        let proposer = self.providers().repair_proposer(now.kind());
        let proposed = match (&proposer, drafts_hold_it) {
            (None, _) => RepairProposal::none(format!(
                "No pack proposes repairs from findings of the kind `{}`.",
                now.kind().as_str()
            )),
            (Some(_), false) => RepairProposal::none(
                "The drafts no longer hold the knowledge that drifted as the published version does: they changed it, or do not compile. A proposal over them would repair what was not published. Lint and publish them, or put the drafts back.",
            ),
            (Some(proposer), true) => proposer.propose(&RepairAsk {
                entry: &asked.knowledge,
                code: &code,
                files: &files,
                before: before.as_ref(),
                now: &now,
            }),
        };
        let mut out = Map::new();
        out.insert("target".to_owned(), json!(target.as_str()));
        out.insert("knowledge".to_owned(), json!(asked.knowledge));
        out.insert(
            "drift".to_owned(),
            json!({
                "tool": held.mark.operation.as_str(),
                "mode": held.mark.mode.as_str(),
                "surface": held.mark.surface.as_str(),
                "code": held.code,
                "run_id": held.run,
                "marked_at": held.found_at,
            }),
        );
        out.insert("stops".to_owned(), Json::Array(stops));
        out.insert("file_kind".to_owned(), json!(kind.as_str()));
        out.insert(
            "files".to_owned(),
            json!(
                files
                    .iter()
                    .map(|file| file.path.as_str())
                    .collect::<Vec<_>>()
            ),
        );
        if let Some(before) = &before {
            out.insert("before".to_owned(), shape_json(before));
        }
        out.insert("now".to_owned(), shape_json(&now));
        let proposal = match proposed {
            RepairProposal::None { reason } => {
                out.insert("proposed".to_owned(), json!(false));
                out.insert("reason".to_owned(), json!(reason));
                None
            }
            RepairProposal::Proposed {
                path,
                text,
                because,
            } => match files.iter().find(|file| file.path == path) {
                Some(file) if file.text != text && text.len() <= LARGEST_TARGET_FILE => {
                    let digest = proposal_digest(&path, &file.text, &text);
                    let diff = line_diff(&file.text, &text);
                    out.insert("proposed".to_owned(), json!(true));
                    out.insert("file".to_owned(), json!(path));
                    out.insert("because".to_owned(), json!(because));
                    out.insert(
                        "diff".to_owned(),
                        Json::Array(
                            diff.iter()
                                .map(|(line, was, is)| {
                                    json!({"line": line, "before": was, "after": is})
                                })
                                .collect(),
                        ),
                    );
                    out.insert("proposal".to_owned(), json!(digest.to_string()));
                    Some(Proposal {
                        path,
                        text,
                        digest,
                        because,
                        diff,
                    })
                }
                _ => {
                    out.insert("proposed".to_owned(), json!(false));
                    out.insert(
                        "reason".to_owned(),
                        json!("The pack's proposer named no change to a file it was given."),
                    );
                    None
                }
            },
        };
        Ok(Bundle {
            json: Json::Object(out),
            proposal,
        })
    }

    /// The proposal `input` names, built again: refused when the bundle
    /// proposes none, or another than the one whose digest `input` gives.
    pub(crate) async fn accepted_repair(
        &self,
        caller: &Caller,
        project: &Path,
        input: &Map<String, Json>,
    ) -> Result<(TargetName, Proposal), Failure> {
        let target = asked(input)?.target;
        let shown: Digest = input
            .get("proposal")
            .and_then(Json::as_str)
            .unwrap_or("")
            .parse()
            .map_err(|_| {
                codes::input(
                    codes::SCHEMA_INVALID,
                    "A proposal is named by the hexadecimal digest `repair.propose` gives.",
                    "Pass the `proposal` of the bundle you read.",
                )
            })?;
        let bundle = self.repair_bundle(caller, project, input).await?;
        let Some(proposal) = bundle.proposal else {
            let why = bundle.json["reason"].as_str().unwrap_or("").to_owned();
            return Err(codes::input(
                codes::REPAIR_NOT_PROPOSED,
                format!("The repair bundle proposes no repair: {why}"),
                "Call `repair.propose` to read why; write the repair as a draft by hand, or give a snapshot it can pair.",
            ));
        };
        if proposal.digest != shown {
            return Err(codes::stale(
                codes::REPAIR_CHANGED,
                "The repair proposed now is not the one shown: the drafts, the drift mark, or the findings changed since.",
                "Call `repair.propose` again, and read the new bundle before accepting it.",
            ));
        }
        Ok((target, proposal))
    }

    /// What a person reads on the page of a repair's acceptance: the file,
    /// each change and why, and what it goes on to need.
    pub(crate) async fn repair_lines(
        &self,
        caller: &Caller,
        project: &(Option<std::path::PathBuf>, Option<Failure>),
        input: &Map<String, Json>,
    ) -> Result<Vec<String>, Failure> {
        let folder = match project {
            (Some(folder), _) => folder.clone(),
            (None, Some(refused)) => return Err(refused.clone()),
            (None, None) => {
                return Err(codes::input(
                    codes::PATH_NOT_ALLOWED,
                    "A repair is written into a project's drafts, and the call names no project folder.",
                    "Run it inside a project folder, or pass `--project` with the folder's full path.",
                ));
            }
        };
        let (target, proposal) = self.accepted_repair(caller, &folder, input).await?;
        let mut lines = vec![format!(
            "The draft `{}` of the target `{target}` is changed as below, and nothing else. Nothing is published: the change is a draft, linted, tested, and published like any other.",
            proposal.path
        )];
        lines.extend(proposal.because.iter().cloned());
        for (line, was, is) in &proposal.diff {
            lines.push(format!(
                "Line {line}: `{}` becomes `{}`.",
                was.trim(),
                is.trim()
            ));
        }
        lines.push(
            "Until a version with the change is published, every tool the drift stopped stays stopped. The words quoted are the page's own, as the snapshots showed them.".to_owned(),
        );
        Ok(lines)
    }
}

handler!(
    /// `repair.propose`: the repair bundle of a drift, built from what is
    /// kept; nothing is written or kept.
    repair_propose,
    |kernel, call| {
        let project = call.project_folder()?.to_path_buf();
        let bundle = kernel
            .repair_bundle(&call.caller, &project, &call.input)
            .await?;
        let output = Output::ok(bundle.json.clone());
        Ok(match bundle.proposal {
            Some(proposal) => {
                let mut asked = call.input.clone();
                asked.insert("proposal".to_owned(), json!(proposal.digest.to_string()));
                output.then(Next::new(
                    "repair.accept",
                    Json::Object(asked),
                    "after reading the diff: the person accepts it on the engine's page, and it is written into the drafts",
                ))
            }
            None => output,
        })
    }
);

handler!(
    /// `repair.accept`: the person writes the repair they were shown into
    /// the drafts, with presence. The dispatcher has asked for the proof,
    /// and sends an agent's call to the page instead.
    repair_accept,
    |kernel, call| {
        if call.presence.is_none() {
            return Err(codes::policy(
                codes::PERSON_REQUIRED,
                "`repair.accept` is a person's act, and no proof of presence was given.",
                "Accept the repair on its page, or at a terminal yourself.",
            ));
        }
        let project = call.project_folder()?.to_path_buf();
        let (target, proposal) = kernel
            .accepted_repair(&call.caller, &project, &call.input)
            .await?;
        let mut input = Map::new();
        input.insert("target".to_owned(), json!(target.as_str()));
        input.insert("file".to_owned(), json!(proposal.path));
        input.insert("content".to_owned(), json!(proposal.text));
        let write = Call {
            input,
            caller: call.caller.clone(),
            decision: call.decision,
            idempotency_key: None,
            project: call.project.clone(),
            project_refused: call.project_refused.clone(),
            presence: None,
            given: None,
            decided: None,
        };
        super::drafts::draft_write(kernel, write).await
    }
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_diff_names_each_changed_line_and_each_line_added_or_removed() {
        assert_eq!(
            line_diff("a\nb\nc", "a\nB\nc"),
            vec![(2, "b".to_owned(), "B".to_owned())]
        );
        assert_eq!(
            line_diff("a\nc", "a\nb\nc"),
            vec![(2, String::new(), "b".to_owned())]
        );
        assert_eq!(
            line_diff("a\nb\nc", "a\nc"),
            vec![(2, "b".to_owned(), String::new())]
        );
        assert!(line_diff("same", "same").is_empty());
    }

    #[test]
    fn a_proposal_is_named_by_its_file_and_both_its_texts() {
        let one = proposal_digest("screens/a.yaml", "x", "y");
        assert_eq!(one, proposal_digest("screens/a.yaml", "x", "y"));
        assert_ne!(one, proposal_digest("screens/b.yaml", "x", "y"));
        assert_ne!(one, proposal_digest("screens/a.yaml", "x", "z"));
        assert_ne!(one, proposal_digest("screens/a.yaml", "xy", ""));
    }
}
