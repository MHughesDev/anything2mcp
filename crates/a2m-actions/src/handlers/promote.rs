//! `binding.promote` (decision 0189): the first binding of a drafted
//! operation made able to act.
//!
//! A drafted binding is `draft`, and a `draft` binding may not act once
//! published. A binding that joins a verified one earns its promotion in
//! shadow; the first binding of an operation has nothing to be compared
//! with, so its evidence is its own: every offline test of the operation
//! passes now, and, for a read, a trial in this project proved it at
//! `executed`. The status is rewritten in the operation's file, and nothing
//! else of it, through the same checked write `draft.write` uses. For an
//! operation that writes, the promote is a person's act; the publish that
//! follows is a person's act whatever the operation does.

use std::path::{Path, PathBuf};

use a2m_core::{BindingStatus, EffectClass, Failure, Mode, OperationName, TargetName, ToolRuns};
use a2m_files::{
    CompileMode, Compiled, DRAFT_VERSION, Doc, Located, RelPath, SourceFile, Sources, TargetRoot,
};
use serde_json::{Value as Json, json};

use super::build::{off_thread, target_folder};
use super::handler;
use crate::codes;
use crate::dispatch::{Call, Next, Output};
use crate::kernel::Kernel;

/// The binding to promote, as the drafts hold it.
struct Found {
    /// The operation's file, inside the target's folder.
    file: RelPath,
    /// The file's text with the binding's status rewritten to `verified`.
    edited: String,
    /// The file's text as it was read: the write is refused where the file
    /// no longer holds it, so that no edit made since is lost.
    original: String,
    /// The drafts compiled as they are.
    compiled: Compiled,
    /// What the operation does.
    effect: EffectClass,
    /// The binding's status now.
    status: BindingStatus,
    /// The binding, by its place, mode, and the digest of what it does:
    /// only evidence that names it is taken (audit finding AC-6).
    pin: a2m_engine::BindingPin,
}

fn not_promotable(message: impl Into<String>, next: impl Into<String>) -> Failure {
    codes::input(codes::BINDING_NOT_PROMOTABLE, message, next)
}

/// The text of `text` with the status of the binding whose `mode` entry is
/// `binding` set to `verified`: the value of its `status` entry replaced on
/// its line, or, with none, a `status` line put after its `mode` line at the
/// same indentation. Nothing else of the file changes; whether the result
/// says what it should is checked by compiling it.
fn with_status_verified(text: &str, binding: &Located<Doc>) -> Option<String> {
    let mut lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
    if let Some(status) = binding.get("status") {
        let line = lines.get_mut(usize::try_from(status.at.line).ok()?.checked_sub(1)?)?;
        let ending = if line.ends_with('\r') { "\r" } else { "" };
        let column = usize::try_from(status.at.column).ok()?.checked_sub(1)?;
        let kept: String = line.chars().take(column).collect();
        *line = format!("{kept}\"verified\"{ending}");
    } else {
        let mode = binding
            .entries()
            .iter()
            .find(|entry| entry.key.value == "mode")?;
        let at = usize::try_from(mode.key.at.line).ok()?.checked_sub(1)?;
        let indent = " ".repeat(usize::try_from(mode.key.at.column).ok()?.checked_sub(1)?);
        let ending = if lines.get(at)?.ends_with('\r') {
            "\r"
        } else {
            ""
        };
        lines.insert(at + 1, format!("{indent}status: \"verified\"{ending}"));
    }
    Some(lines.join("\n"))
}

/// Finds the binding of `mode` of the drafted operation `tool`, checks that
/// it is one `binding.promote` may make verified, and makes the edited text,
/// checked by compiling the drafts with it: the operation must compile to
/// the same plan but for that one status.
#[allow(
    clippy::too_many_lines,
    reason = "one search through the drafts, with a refusal at each step"
)]
fn locate(
    kernel: &Kernel,
    project_folder: &Path,
    target: &TargetName,
    tool: &OperationName,
    mode: &Mode,
    position: Option<usize>,
) -> Result<Found, Failure> {
    let set = kernel.providers().compiler_set();
    let folder = target_folder(project_folder, target)?;
    if !folder.is_dir() {
        return Err(codes::input(
            codes::NOT_FOUND,
            format!("The project has no target named `{target}`."),
            "Call `target.list` to see the project's targets.",
        ));
    }
    let root = TargetRoot::new(folder.as_path());
    let sources = a2m_files::load(&root, set).map_err(Failure::from)?;
    let compiled = a2m_files::compile(&sources, set, CompileMode::Draft, DRAFT_VERSION)
        .map_err(Failure::from)?;
    let plan = &compiled.plan;
    let no_tool = || {
        codes::input(
            codes::NOT_FOUND,
            format!("The drafts of `{target}` hold no operation named `{tool}`."),
            "Call `draft.lint` to list the drafted tools.",
        )
    };
    let Some(ToolRuns::Operation(index)) = plan.tool(tool).map(|found| found.runs) else {
        return Err(no_tool());
    };
    let operation = plan.operation(index).ok_or_else(no_tool)?;
    let of_mode: Vec<usize> = operation
        .bindings
        .iter()
        .enumerate()
        .filter(|(at, binding)| binding.mode == *mode && position.is_none_or(|given| given == *at))
        .map(|(position, _)| position)
        .collect();
    let position = match of_mode.as_slice() {
        [one] => *one,
        [] => {
            return Err(codes::input(
                codes::NOT_FOUND,
                format!("The operation `{tool}` has no binding of the mode `{mode}`."),
                "Name a mode the operation's bindings state.",
            ));
        }
        _ => {
            return Err(not_promotable(
                format!(
                    "The operation `{tool}` has more than one binding of the mode `{mode}`, and nothing says which is meant."
                ),
                "Name it by `position` as well, counted from 1 among the operation's bindings.",
            ));
        }
    };
    let pin = a2m_engine::BindingPin::of(plan, operation, position).ok_or_else(no_tool)?;
    let status = operation.bindings[position].status;
    match status {
        BindingStatus::Draft | BindingStatus::Verified => {}
        BindingStatus::Shadow => {
            return Err(codes::input(
                codes::ACTION_NOT_BUILT,
                format!(
                    "The binding of `{mode}` of `{tool}` is `shadow`: it is promoted on its exact matches against the verified binding, and that comparison is not built."
                ),
                "Keep it in shadow; `shadow.report` and its promotion come with the shadow comparison.",
            ));
        }
        BindingStatus::Retired => {
            return Err(not_promotable(
                format!(
                    "The binding of `{mode}` of `{tool}` is `retired`, and a retired binding never acts again."
                ),
                "Write a new binding.",
            ));
        }
    }
    if status == BindingStatus::Draft
        && operation
            .bindings
            .iter()
            .any(|binding| binding.status == BindingStatus::Verified)
    {
        return Err(not_promotable(
            format!(
                "The operation `{tool}` already has a verified binding: a binding beside it is promoted from shadow, on its exact matches, not as a first binding."
            ),
            "Set the binding's status to `shadow` and let it be compared.",
        ));
    }
    // Its place in its file.
    let in_file = sources
        .of_kind("operation")
        .flat_map(|file| file.docs.iter().map(move |doc| (file, doc)))
        .find(|(_, doc)| doc.text("operation") == Some(tool.as_str()));
    let Some((file, doc)) = in_file else {
        return Err(no_tool());
    };
    let path = file.path.clone();
    let in_file = doc.get("bindings").map(Located::items).unwrap_or_default();
    let at_place: Vec<&Located<Doc>> = in_file
        .get(position)
        .filter(|binding| binding.text("mode") == Some(mode.as_str()))
        .into_iter()
        .collect();
    let [binding] = at_place.as_slice() else {
        return Err(not_promotable(
            format!("The binding of `{mode}` of `{tool}` cannot be found once in `{path}`."),
            "Promote it by writing its status in the operation's file.",
        ));
    };
    let bytes = folder
        .join(path.as_str())?
        .read(a2m_files::LARGEST_TARGET_FILE as u64)?;
    let text = String::from_utf8(bytes).map_err(|_| {
        not_promotable(
            format!("`{path}` is not UTF-8 text."),
            "Save the file as UTF-8.",
        )
    })?;
    let cannot = || {
        not_promotable(
            format!(
                "The status of the binding of `{mode}` of `{tool}` cannot be rewritten by rule in `{path}`: the file writes it in a form the rewrite does not keep."
            ),
            "Write `status: verified` in the binding yourself, with `draft.write`.",
        )
    };
    let edited = with_status_verified(&text, binding).ok_or_else(cannot)?;
    // The edited file must compile to the same operation but for the one
    // status.
    let read = SourceFile::read(path.clone(), edited.as_bytes(), set).map_err(|_| cannot())?;
    let files: Vec<SourceFile> = sources
        .files
        .iter()
        .map(|file| {
            if file.path == path {
                read.clone()
            } else {
                file.clone()
            }
        })
        .collect();
    let edited_sources = Sources::from_files(root, files);
    let recompiled = a2m_files::compile(&edited_sources, set, CompileMode::Draft, DRAFT_VERSION)
        .map_err(|_| cannot())?;
    let same = recompiled
        .plan
        .tool(tool)
        .and_then(|found| match found.runs {
            ToolRuns::Operation(index) => recompiled.plan.operation(index),
            ToolRuns::Pipeline(_) => None,
        })
        .is_some_and(|after| {
            let mut after = after.clone();
            let promoted = after
                .bindings
                .get(position)
                .is_some_and(|binding| binding.status == BindingStatus::Verified);
            if let Some(binding) = after.bindings.get_mut(position) {
                binding.status = status;
            }
            promoted && after == *operation
        });
    if !same {
        return Err(cannot());
    }
    Ok(Found {
        file: path,
        edited,
        original: text,
        effect: operation.effect,
        status,
        compiled,
        pin,
    })
}

/// The evidence of the operation's offline tests (audit findings AC-6 and
/// PM-4): every offline test of it, run now with the binding being promoted
/// standing as the one that would act, must pass; at least one must have
/// run that binding, with the digest it has now; and, for an operation
/// that writes, one of those must have sent its save through it and
/// compared it. A test of another binding is evidence for that one alone.
async fn offline_evidence(
    kernel: &Kernel,
    call: &Call,
    project_folder: PathBuf,
    target: &TargetName,
    found: &Found,
) -> Result<Vec<Json>, Failure> {
    let tool = &found.pin.operation;
    let world = crate::offline::OfflineWorld {
        workspace: call.caller.principal.workspace,
        principal: call.caller.principal.principal.id,
        vault: kernel.inner.vault.clone(),
    };
    let promoting = crate::offline::Promoting {
        operation: tool.clone(),
        position: found.pin.position,
    };
    let gates = crate::offline::run_gates(
        kernel,
        project_folder,
        target,
        &found.compiled,
        world,
        Some(promoting),
    )
    .await?;
    let tests: Vec<Json> = gates
        .gates
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|gate| gate["tests"].as_array())
        .flatten()
        .filter(|test| test["tool"] == tool.as_str() && test["kind"] == "offline")
        .map(|test| {
            json!({
                "test": test["test"],
                "state": test["state"],
                "binding": test["binding"],
                "path": test["path"],
            })
        })
        .collect();
    if tests.is_empty() {
        return Err(codes::input(
            codes::PROMOTION_UNPROVEN,
            format!(
                "The operation `{tool}` has no offline test, so nothing shows its binding gives what it should."
            ),
            "Write an offline test with a fixture of synthetic values (`draft.write`), run `test.run`, and promote again.",
        ));
    }
    let failing: Vec<String> = tests
        .iter()
        .filter(|test| test["state"] != "passed")
        .map(|test| {
            format!(
                "{} ({})",
                test["test"].as_str().unwrap_or_default(),
                test["state"].as_str().unwrap_or_default()
            )
        })
        .collect();
    if !failing.is_empty() {
        return Err(codes::input(
            codes::PROMOTION_UNPROVEN,
            format!(
                "These offline tests of `{tool}` did not pass now: {}.",
                failing.join(", ")
            ),
            format!("Call `test.run` with the target `{target}` to see what each found."),
        ));
    }
    let of_this = |test: &&Json| {
        a2m_engine::BindingPin::from_json(tool, &test["binding"]).as_ref() == Some(&found.pin)
    };
    let shown = format!("{} (`{}`)", found.pin.position + 1, found.pin.mode);
    if !tests.iter().any(|test| of_this(&test)) {
        return Err(codes::input(
            codes::PROMOTION_UNPROVEN,
            format!(
                "No offline test of `{tool}` ran its binding {shown}: the tests that passed are evidence for the bindings they ran."
            ),
            format!(
                "Write a test of that binding, with `binding: {{mode: {}, position: {}}}` where it is not the one a test that names none runs, run `test.run`, and promote again.",
                found.pin.mode,
                found.pin.position + 1
            ),
        ));
    }
    if found.effect != EffectClass::Read
        && !tests
            .iter()
            .any(|test| of_this(&test) && test["path"] == crate::offline::PATH_SENDS)
    {
        return Err(codes::input(
            codes::PROMOTION_UNPROVEN,
            format!(
                "No offline test of `{tool}` sent its save through the binding {shown} and compared it: a write is proven by its save."
            ),
            "Add a test of that binding whose fixture records the save's request in `<step>.request.json`, run `test.run`, and promote again.",
        ));
    }
    Ok(tests)
}

/// The proof that a read was tried and proved in this project (audit
/// findings AC-6 and PM-3): the trial the caller names, which must be the
/// caller's own completed trial of this tool of this target, made from this
/// project, of the binding being promoted with the digest it has now; or
/// the newest finished scout's proof at `executed` of that binding with
/// that digest.
async fn trial_evidence(
    kernel: &Kernel,
    call: &Call,
    target: &TargetName,
    found: &Found,
) -> Result<Json, Failure> {
    let tool = &found.pin.operation;
    let of_this = |stored: &Json| {
        a2m_engine::BindingPin::from_json(tool, stored).as_ref() == Some(&found.pin)
    };
    if let Some(given) = call.text("run_id") {
        let unproven = || {
            codes::input(
                codes::PROMOTION_UNPROVEN,
                format!(
                    "The run named is not a completed trial of the binding {} (`{}`) of `{tool}` of `{target}`, as it is now, made from this project by this caller's person.",
                    found.pin.position + 1,
                    found.pin.mode
                ),
                "Try that binding with `draft.try` (naming it by `mode` and `position`) and pass its `run_id`, or scout the target again.",
            )
        };
        let run: a2m_core::RunId = given.parse().map_err(|_| unproven())?;
        let read = kernel
            .store()?
            .read(move |tx| {
                let row = tx.ledger().run(run)?;
                let stored = crate::runner::run_call(tx, &row)?;
                Ok((row, stored))
            })
            .await
            .map_err(|_| unproven())?;
        let (row, stored) = read;
        let own = row.workspace == call.caller.principal.workspace
            && call.caller.principal.person() == Some(row.principal);
        let Some(stored) = stored else {
            return Err(unproven());
        };
        // A trial carries the binding it ran; a call of the published tool
        // carries none, and is no trial of a draft.
        let ran_this = stored.caller.binding.as_ref().is_some_and(of_this);
        let same_project = stored.caller.project.is_some() && stored.caller.project == call.project;
        if !own
            || !ran_this
            || !same_project
            || row.tool_name != *tool
            || row.target != *target
            || row.state != a2m_core::RunState::Completed
        {
            return Err(unproven());
        }
        return Ok(json!({
            "by": "run",
            "run_id": run.to_string(),
            "level": "executed",
            "binding": found.pin.to_json(),
        }));
    }
    super::scout::proven_by_scout(kernel, call, target, tool.as_str())
        .await?
        .filter(|proof| of_this(&proof["binding"]))
        .ok_or_else(|| {
            codes::input(
                codes::PROMOTION_UNPROVEN,
                format!(
                    "No trial in this project proved the binding {} (`{}`) of the read `{tool}`, as it is now, at `executed`: the newest finished scout did not, and no run was named.",
                    found.pin.position + 1,
                    found.pin.mode
                ),
                "Scout the target (`scout.run`), or try that binding (`draft.try` with `mode` and `position`) and pass its `run_id`.",
            )
        })
}

handler!(
    /// `binding.promote`: the first binding of a drafted operation made
    /// verified, on its evidence, through the checked write `draft.write`
    /// uses; for an operation that writes, as a person's act.
    binding_promote,
    |kernel, call| {
        let target = TargetName::new(call.required("target")?)?;
        let tool = OperationName::new(call.required("tool")?)?;
        let mode = Mode::new(call.required("mode")?)?;
        let position = call
            .int("position")
            .and_then(|position| usize::try_from(position).ok())
            .and_then(|position| position.checked_sub(1));
        let project_folder = call.project_folder()?.to_path_buf();
        let found = {
            let (kernel, folder, target, tool, mode) = (
                kernel.clone(),
                project_folder.clone(),
                target.clone(),
                tool.clone(),
                mode.clone(),
            );
            off_thread(move || locate(&kernel, &folder, &target, &tool, &mode, position)).await?
        };
        let shown = |promoted: bool, status: &str, evidence: Json| {
            json!({
                "target": target.as_str(),
                "tool": tool.as_str(),
                "mode": mode.as_str(),
                "position": found.pin.position + 1,
                "file": found.file.as_str(),
                "promoted": promoted,
                "status": status,
                "evidence": evidence,
            })
        };
        if found.status == BindingStatus::Verified {
            return Ok(Output::ok(shown(false, "verified", json!({})))
                .warn("The binding is verified already; nothing was changed."));
        }
        // An operation that writes: the promote is a person's act. An agent
        // is handed to the person's page; a caller at a terminal is asked
        // for presence; the person's press runs this again with the proof.
        if found.effect != EffectClass::Read && call.presence.is_none() {
            let def = kernel
                .actions()
                .find("binding.promote")
                .cloned()
                .ok_or_else(|| {
                    codes::input(
                        codes::ACTION_NOT_BUILT,
                        "`binding.promote` is not in this engine's registry.",
                        "Call `action.search` to see which actions are built.",
                    )
                })?;
            if call.caller.is_agent {
                let project = (call.project.clone(), call.project_refused.clone());
                return kernel
                    .ask_person(&def, &call.caller, call.input.clone(), project)
                    .await;
            }
            kernel.presence(&call.caller, &def, found.effect).await?;
        }
        let tests = offline_evidence(kernel, &call, project_folder.clone(), &target, &found).await?;
        let tried = if found.effect == EffectClass::Read {
            trial_evidence(kernel, &call, &target, &found).await?
        } else {
            json!({"by": "person", "why": "A write is not tried by the scout; its offline tests and the person's act are its evidence."})
        };
        let evidence = json!({
            "binding": found.pin.to_json(),
            "offline_tests": tests,
            "tried": tried,
            "was": "draft",
        });
        // The same checked write as `draft.write`: through the checked path
        // of the project, a new file renamed over the name, never through a
        // link.
        {
            let (folder, target, path, edited, original) = (
                project_folder.clone(),
                target.clone(),
                found.file.clone(),
                found.edited.clone(),
                found.original.clone(),
            );
            off_thread(move || {
                let folder = target_folder(&folder, &target)?;
                let file = folder.join(path.as_str())?;
                if !matches!(file.entry_kind()?, a2m_provider::EntryKind::File) {
                    return Err(not_promotable(
                        format!("`{path}` is no longer a file of the target."),
                        "Call `draft.lint`, then promote again.",
                    ));
                }
                // An edit made since the binding was found is never lost.
                if file.read(a2m_files::LARGEST_TARGET_FILE as u64)? != original.as_bytes() {
                    return Err(codes::stale(
                        codes::DRAFTS_CHANGED,
                        format!("`{path}` changed while the binding was promoted, so nothing was written."),
                        "Promote again.",
                    ));
                }
                file.write(edited.as_bytes())
            })
            .await?;
        }
        let detail = format!("{target} {tool} {mode}: draft to verified, on {}", evidence);
        kernel
            .audit(
                &call.caller.principal.principal.id.to_string(),
                "binding.promote",
                &format!("{target}/{tool}"),
                &detail,
            )
            .await?;
        Ok(
            Output::ok(shown(true, "verified", evidence)).then(Next::new(
                "publish.request",
                json!({"target": target.as_str()}),
                "to have a person publish the version in which the binding first acts",
            )),
        )
    }
);
