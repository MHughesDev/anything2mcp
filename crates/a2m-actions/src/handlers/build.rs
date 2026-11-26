//! Building a target: its draft files in the project folder, and the
//! version a person publishes from them.
//!
//! This crate is the only caller of the compiler. A compiled target is
//! produced here and stored by `version.publish`; the engine receives it
//! through `Control` and never sees the compiler.

use a2m_core::{Digest, EncodedCompiledTarget, Failure, PrincipalId, TargetName, Version};
use a2m_files::{CompileMode, Compiled, DRAFT_VERSION, Diagnostics, TargetRoot};
use a2m_ledger::{Durability, PublishedVersion};
use a2m_provider::{Access, CheckedPath, RootName, Roots};
use serde_json::{Value as Json, json};
use std::path::Path;

use super::handler;
use crate::codes;
use crate::dispatch::{Call, ExitClass, Next, Output};
use crate::kernel::{Kernel, note_published};

/// The folder of a project that holds one folder per target.
pub(crate) const TARGETS_FOLDER: &str = "targets";

/// The project folder of a call, as a checked path. The folder a local
/// command was run in is the one root a project's files may be read from
/// and written to. Looks at the disk.
pub(crate) fn project(folder: &Path) -> Result<CheckedPath, Failure> {
    let name = RootName::new("project")?;
    let mut roots = Roots::none();
    roots.add(name.clone(), folder, Access::Write)?;
    roots.folder(&name, Access::Write)
}

/// The folder of one target of the project. Looks at the disk.
pub(crate) fn target_folder(
    project_folder: &Path,
    target: &TargetName,
) -> Result<CheckedPath, Failure> {
    project(project_folder)?.join(&format!("{TARGETS_FOLDER}/{target}"))
}

pub(crate) fn problems_json(problems: &Diagnostics) -> Vec<Json> {
    match problems.to_json() {
        Json::Array(items) => items,
        _ => Vec::new(),
    }
}

/// Loads and compiles the target's drafts. Reads every draft file and
/// compiles them, so it is called off the async threads.
fn compile_target(
    kernel: &Kernel,
    folder: &CheckedPath,
    mode: CompileMode,
    version: Version,
    tools: Option<&[String]>,
) -> Result<Result<Compiled, Diagnostics>, Failure> {
    let set = kernel.providers().compiler_set();
    let sources = match a2m_files::load(&TargetRoot::new(folder.as_path()), set) {
        Ok(sources) => sources,
        Err(problems) => return Ok(Err(problems)),
    };
    let sources = match tools {
        Some(tools) => chosen_sources(sources, tools)?,
        None => sources,
    };
    Ok(a2m_files::compile(&sources, set, mode, version))
}

/// The tools a call names under `tools`, in its order: `None` when it
/// names none, and every drafted tool is meant.
pub(crate) fn chosen_tools(call: &Call) -> Option<Vec<String>> {
    call.input
        .get("tools")
        .and_then(Json::as_array)
        .map(|list| {
            list.iter()
                .filter_map(Json::as_str)
                .map(str::to_owned)
                .collect()
        })
}

/// The name a file of an operation, a pipeline, or a test is of: what each
/// of its documents names under the kind's own key. `None` for a file of
/// another kind.
fn names_in(file: &a2m_files::SourceFile) -> Option<Vec<&str>> {
    let key = match file.kind.as_str() {
        a2m_files::kernel::OPERATION => "operation",
        a2m_files::kernel::PIPELINE => "pipeline",
        a2m_files::kernel::TEST => "test",
        _ => return None,
    };
    Some(file.docs.iter().filter_map(|doc| doc.text(key)).collect())
}

/// The sources a publish of only `tools` compiles (decision 0457): every
/// file but each operation and pipeline file that defines no tool named,
/// each test of a tool not named, and each fixture only those tests name.
/// The other files stay whole, so what a named tool needs is compiled as
/// the drafts hold it, and a file is never cut: one that defines a tool
/// named and one not named refuses the call.
///
/// # Errors
///
/// `input`/`not_found` for a name no operation or pipeline file defines;
/// `input`/`schema_invalid` for a file that defines both a tool named and
/// one not named.
pub(crate) fn chosen_sources(
    sources: a2m_files::Sources,
    tools: &[String],
) -> Result<a2m_files::Sources, Failure> {
    let named = |name: &str| tools.iter().any(|tool| tool == name);
    for tool in tools {
        let defined = sources
            .files
            .iter()
            .filter(|file| file.kind.as_str() != a2m_files::kernel::TEST)
            .filter_map(names_in)
            .any(|names| names.contains(&tool.as_str()));
        if !defined {
            return Err(codes::input(
                codes::NOT_FOUND,
                format!(
                    "The drafts hold no tool named `{}`, so nothing was compiled.",
                    codes::shown(tool)
                ),
                "Call `draft.lint` to list the drafted tools, and name only those under `tools`.",
            ));
        }
    }
    // The fixtures the tests left in name, and those the tests left out.
    let (mut kept_fixtures, mut dropped_fixtures) = (Vec::new(), Vec::new());
    for file in sources.of_kind(a2m_files::kernel::TEST) {
        for doc in &file.docs {
            if let Some(fixture) = doc.text("fixture") {
                if doc.text("test").is_some_and(named) {
                    kept_fixtures.push(fixture.to_owned());
                } else {
                    dropped_fixtures.push(fixture.to_owned());
                }
            }
        }
    }
    let mut kept = Vec::with_capacity(sources.files.len());
    for file in sources.files {
        if file.kind.as_str() == a2m_files::kernel::FIXTURE {
            let folder = file
                .path
                .as_str()
                .strip_prefix("fixtures/")
                .and_then(|rest| rest.split_once('/'))
                .map(|(folder, _)| folder.to_owned());
            if folder.is_some_and(|folder| {
                dropped_fixtures.contains(&folder) && !kept_fixtures.contains(&folder)
            }) {
                continue;
            }
            kept.push(file);
            continue;
        }
        let Some(names) = names_in(&file) else {
            kept.push(file);
            continue;
        };
        let (wanted, unwanted) = names.iter().partition::<Vec<&str>, _>(|name| named(name));
        match (wanted.is_empty(), unwanted.is_empty()) {
            (false, false) => {
                return Err(codes::input(
                    codes::SCHEMA_INVALID,
                    format!(
                        "`{}` defines `{}`, which is named under `tools`, and `{}`, which is not; a publish of some tools leaves whole files out and never cuts one.",
                        file.path.as_str(),
                        codes::shown(wanted[0]),
                        codes::shown(unwanted[0])
                    ),
                    "Name every tool of the file under `tools`, or move the one not wanted into a file of its own.",
                ));
            }
            (false, true) => kept.push(file),
            // A file with no document named, and one of tools not named,
            // is left out.
            (true, _) if names.is_empty() => kept.push(file),
            (true, _) => {}
        }
    }
    Ok(a2m_files::Sources::from_files(sources.root, kept))
}

/// Compiles what of a target's drafts can compile, for observing and trying
/// them: every file but those `held_back` names and those that hold a
/// drafter's open question, which keep their whole kind from compiling
/// (decision 0180). Drafts that hold a question now wait in the holding
/// folder, which the loader does not read (decision 0298), so they are not
/// among the files; a file outside it that holds one, written before that
/// decision or by a person, is still left out here, as it was. Gives the
/// compiled target and the files left out, in order. Never used to publish: a
/// publish compiles every file.
pub(crate) fn compile_what_can(
    kernel: &Kernel,
    folder: &std::path::Path,
    held_back: &[String],
) -> Result<(Compiled, Vec<String>), Diagnostics> {
    let set = kernel.providers().compiler_set();
    let root = TargetRoot::new(folder);
    let sources = a2m_files::load(&root, set)?;
    let compile = |left_out: &std::collections::BTreeSet<String>| {
        let files = sources
            .files
            .iter()
            .filter(|file| !left_out.contains(file.path.as_str()))
            .cloned()
            .collect();
        a2m_files::compile(
            &a2m_files::Sources::from_files(root.clone(), files),
            set,
            CompileMode::Draft,
            a2m_files::DRAFT_VERSION,
        )
    };
    let mut left_out: std::collections::BTreeSet<String> = held_back.iter().cloned().collect();
    match compile(&left_out) {
        Ok(compiled) => Ok((compiled, left_out.into_iter().collect())),
        Err(problems) => {
            let questioned: Vec<String> = problems
                .iter()
                .filter(|problem| problem.code.as_str() == a2m_files::codes::OPEN_QUESTION)
                .map(|problem| problem.at.file.as_str().to_owned())
                .collect();
            if questioned.iter().all(|file| left_out.contains(file)) {
                return Err(problems);
            }
            left_out.extend(questioned);
            compile(&left_out).map(|compiled| (compiled, left_out.into_iter().collect()))
        }
    }
}

/// The drafts waiting in the target's holding folder, ordered by path.
/// Looks at the disk, so it is called off the async threads.
pub(crate) async fn held_drafts(
    call: &Call,
    target: &TargetName,
) -> Result<a2m_files::Held, Failure> {
    let project_folder = call.project_folder()?.to_path_buf();
    let target = target.clone();
    off_thread(move || {
        let folder = target_folder(&project_folder, &target)?;
        Ok(a2m_files::held_files(&TargetRoot::new(folder.as_path())))
    })
    .await
}

/// The held drafts as an answer shows them: each file by its path, with its
/// open questions and where each stands. The question is the drafter's own
/// words and may quote the system's, so it is marked untrusted whole, as a
/// question the ledger holds in one piece is.
pub(crate) fn held_json(held: &a2m_files::Held) -> Vec<Json> {
    held.files
        .iter()
        .map(|file| {
            json!({
                "file": file.path.as_str(),
                "questions": file
                    .questions
                    .iter()
                    .map(|question| json!({
                        "question": {"untrusted": question.text},
                        "at": question.at.to_string(),
                    }))
                    .collect::<Vec<_>>(),
            })
        })
        .collect()
}

/// What an answer says, in the product's own words, when the compile has
/// failed and the target has held drafts: a name that was not found may be
/// defined by one of them. No file is named and no diagnostic is changed.
pub(crate) fn held_note(held: &a2m_files::Held) -> Option<String> {
    if held.files.is_empty() {
        return None;
    }
    let count = if held.cut {
        format!("{}{}", held.files.len(), " or more")
    } else {
        held.files.len().to_string()
    };
    Some(format!(
        "This target has {count} held drafts. A name that was not found may be defined by one of them; `draft.lint` lists them."
    ))
}

/// The failure with the note of [`held_note`] after its message, when the
/// target has held drafts.
pub(crate) fn noting_held(failure: Failure, held: &a2m_files::Held) -> Failure {
    match held_note(held) {
        Some(note) => Failure::new(
            failure.class(),
            failure.code().clone(),
            format!("{} {note}", failure.message()),
            failure.next_step(),
        )
        .carrying_said(&failure),
        None => failure,
    }
}

/// The refusal of a tool whose draft is held: `None` when no held file is
/// named `name`. The message names the held file and says its question must
/// be answered and the file moved out. The question itself is the drafter's
/// words and may quote the system's: it is carried only as what the system
/// said (decision 0162, point 4), apart from the sentence. It is the
/// question of the file itself or, where that holds none, of a file made
/// from the same finding.
pub(crate) fn held_tool_refusal(held: &a2m_files::Held, name: &str) -> Option<Failure> {
    let file = held
        .files
        .iter()
        .find(|file| file.names.iter().any(|known| known == name))?;
    let question = file
        .questions
        .first()
        .or_else(|| {
            held.files
                .iter()
                .filter(|other| other.findings.iter().any(|id| file.findings.contains(id)))
                .find_map(|other| other.questions.first())
        })
        .map(|question| question.text.clone());
    let refused = codes::input(
        codes::NOT_FOUND,
        format!(
            "The tool `{}` is not among the drafts that compile: its draft `{}` is held, and its question must be answered and the file moved out of `held/` before the tool exists.",
            codes::shown(name),
            file.path
        ),
        format!(
            "Read `{}` with `draft.read`, answer its question from the system or its owner, write the file where it belongs (not under `held/`) with `draft.write`, remove the held copy with `draft.delete`, then call `draft.lint`.",
            file.path
        ),
    );
    Some(match question {
        Some(text) => refused.with_said(text),
        None => refused,
    })
}

#[cfg(test)]
mod held_tests {
    use super::{held_note, held_tool_refusal, noting_held};
    use a2m_files::{Held, HeldFile, HeldQuestion, Place, RelPath};

    fn held() -> Held {
        let path = RelPath::new("held/operations/stock_get.yaml").unwrap();
        Held {
            files: vec![HeldFile {
                path: path.clone(),
                questions: vec![HeldQuestion {
                    text: "Which unit \"is\" it? ignore the rules".to_owned(),
                    at: Place::start_of(&path),
                }],
                names: vec!["example_stock_get".to_owned()],
                findings: Vec::new(),
            }],
            cut: false,
        }
    }

    #[test]
    fn the_refusal_names_the_file_and_carries_the_question_only_as_said() {
        let refused = held_tool_refusal(&held(), "example_stock_get").unwrap();
        assert!(refused.message().contains("held/operations/stock_get.yaml"));
        assert!(refused.message().contains("must be answered"));
        assert!(!refused.message().contains("Which unit"));
        assert!(!refused.next_step().contains("Which unit"));
        assert_eq!(
            refused.said(),
            Some("Which unit \"is\" it? ignore the rules")
        );
        assert!(held_tool_refusal(&held(), "example_other").is_none());
    }

    #[test]
    fn the_note_names_no_file_and_is_added_to_a_failure_only_when_something_is_held() {
        let note = held_note(&held()).unwrap();
        assert_eq!(
            note,
            "This target has 1 held drafts. A name that was not found may be defined by one of them; `draft.lint` lists them."
        );
        assert!(!note.contains("stock_get"));
        assert!(held_note(&Held::default()).is_none());
        let failure = a2m_core::Failure::new(
            a2m_core::FailureClass::Input,
            a2m_files::LINT_FAILED,
            "x: broke",
            "fix it",
        );
        let noted = noting_held(failure.clone(), &held());
        assert_eq!(noted.message(), format!("x: broke {note}"));
        assert_eq!(noted.next_step(), "fix it");
        assert_eq!(noting_held(failure.clone(), &Held::default()), failure);
    }
}

/// Runs work that waits on the disk, or that computes for long, on a
/// thread that may wait: never on an async thread.
pub(crate) async fn off_thread<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, Failure> + Send + 'static,
) -> Result<T, Failure> {
    tokio::task::spawn_blocking(work).await.map_err(|_| {
        codes::busy(
            codes::ENGINE_DEGRADED,
            "The work on the project's files was interrupted.",
            "Try again.",
        )
    })?
}

/// Finds the target's folder in the project and compiles its drafts, off
/// the async threads.
async fn compiled_target(
    kernel: &Kernel,
    call: &Call,
    target: &TargetName,
    mode: CompileMode,
    version: Version,
) -> Result<Result<Compiled, Diagnostics>, Failure> {
    let project_folder = call.project_folder()?.to_path_buf();
    compile_chosen_in(
        kernel,
        project_folder,
        target,
        mode,
        version,
        chosen_tools(call),
    )
    .await
}

/// Compiles the drafts of `target` in the checked project folder, off the
/// async threads.
pub(crate) async fn compile_in(
    kernel: &Kernel,
    project_folder: std::path::PathBuf,
    target: &TargetName,
    mode: CompileMode,
    version: Version,
) -> Result<Result<Compiled, Diagnostics>, Failure> {
    compile_chosen_in(kernel, project_folder, target, mode, version, None).await
}

/// As [`compile_in`], of only the tools `tools` names, when it names some
/// ([`chosen_sources`]).
pub(crate) async fn compile_chosen_in(
    kernel: &Kernel,
    project_folder: std::path::PathBuf,
    target: &TargetName,
    mode: CompileMode,
    version: Version,
    tools: Option<Vec<String>>,
) -> Result<Result<Compiled, Diagnostics>, Failure> {
    let (kernel, target) = (kernel.clone(), target.clone());
    off_thread(move || {
        let folder = target_folder(&project_folder, &target)?;
        compile_target(&kernel, &folder, mode, version, tools.as_deref())
    })
    .await
}

/// The next step for drafts that compile, chosen on the gates a publish
/// runs (decision 0219): the same publish compile, which refuses an
/// operation with no offline test, and the same offline tests. Only drafts
/// that pass both are pointed at `ready`, the publish; otherwise the hint
/// names the step that can succeed. Nothing is stored: an offline test runs
/// in a store of its own.
/// The `binding.promote` the drafts need before a publish, when the binding
/// that would act for an operation is a `draft`: a published `draft`
/// binding may not act, so a `next` that skipped it would lead to a version
/// whose tools cannot run (package 5.6b). It names the first such
/// operation, and its `when` names them all.
pub(crate) fn promote_next(target: &TargetName, compiled: &Compiled) -> Option<Next> {
    let waiting: Vec<(&a2m_core::CompiledOperation, &a2m_core::Mode)> = compiled
        .compiled_target
        .operations
        .iter()
        .filter_map(|operation| {
            let binding =
                a2m_engine::default_binding(operation).and_then(|at| operation.bindings.get(at))?;
            (binding.status == a2m_core::BindingStatus::Draft).then_some((operation, &binding.mode))
        })
        .collect();
    let (first, mode) = waiting.first()?;
    let names: Vec<&str> = waiting
        .iter()
        .map(|(operation, _)| operation.name.as_str())
        .collect();
    Some(Next::new(
        "binding.promote",
        json!({"target": target.as_str(), "tool": first.name.as_str(), "mode": mode.as_str()}),
        &format!(
            "before the publish: a published `draft` binding may not act, and the binding that would act is a draft for {}. For a read the newest scout did not prove, add the `run_id` of a completed `draft.try` of it. `draft.ready` takes every tried read through these steps at once",
            names.join(", ")
        ),
    ))
}

/// The `when` of `draft.ready` as the way on from a place on the first
/// hour's path (decision 0485, point 2).
pub(crate) const READY_WHEN: &str = "to record the offline test and fixture of each read the scout proved, promote its binding, run the publish gates, and ask a person to publish, in one action; `tools` names the reads to publish when not all are wanted";

/// `draft.ready` as the one way on from a place on the first hour's path
/// (decision 0485, point 2): when the newest scout of the target proved a
/// read and the target has no published version yet, every step between
/// here and the publish is that one action, whatever the step just taken
/// was. `None` otherwise, and the caller names its own next step.
pub(crate) async fn ready_way(kernel: &Kernel, call: &Call, target: &TargetName) -> Option<Next> {
    let workspace = call.caller.principal.workspace;
    let listed = target.clone();
    let published = kernel
        .store()
        .ok()?
        .read(move |tx| tx.versions().list(&workspace, &listed))
        .await
        .ok()?
        .iter()
        .any(|row| row.current);
    if published {
        return None;
    }
    let scout = super::scout::scout_tried(kernel, call, target)
        .await
        .ok()??;
    scout
        .entries
        .iter()
        .any(|entry| entry["state"] == "proven")
        .then(|| {
            Next::new(
                "draft.ready",
                json!({"target": target.as_str()}),
                READY_WHEN,
            )
        })
}

pub(crate) async fn publish_next(
    kernel: &Kernel,
    call: &Call,
    target: &TargetName,
    ready: Next,
) -> Next {
    let tests = |when: String| Next::new("test.run", json!({"target": target.as_str()}), &when);
    let Ok(project_folder) = call.project_folder().map(std::path::Path::to_path_buf) else {
        return ready;
    };
    let compiled = match compile_in(
        kernel,
        project_folder.clone(),
        target,
        CompileMode::Publish,
        DRAFT_VERSION,
    )
    .await
    {
        Ok(Ok(compiled)) => compiled,
        Ok(Err(problems)) => {
            let untested = crate::offline::without_test(target, &problems);
            if untested.is_empty() {
                return Next::new(
                    "draft.lint",
                    json!({"target": target.as_str()}),
                    "after correcting what the publish compile refuses",
                );
            }
            // Reads the newest scout proved are made ready by one action
            // (package 5.6c), which leaves the rest alone with its reason.
            if let Ok(Some(scout)) = super::scout::scout_tried(kernel, call, target).await
                && scout.entries.iter().any(|entry| entry["state"] == "proven")
            {
                return Next::new(
                    "draft.ready",
                    json!({"target": target.as_str()}),
                    READY_WHEN,
                );
            }
            let files: Vec<&str> = untested
                .iter()
                .filter_map(|entry| entry["file"].as_str())
                .collect();
            return tests(format!(
                "after writing an offline test with a fixture for each operation that has none ({}), or removing the drafts not wanted with `draft.delete`: a publish needs a passing test of every operation",
                files.join(", ")
            ));
        }
        Err(_) => return tests("to see whether the publish gates pass".to_owned()),
    };
    let world = crate::offline::OfflineWorld {
        workspace: call.caller.principal.workspace,
        principal: call.caller.principal.principal.id,
        vault: kernel.inner.vault.clone(),
    };
    match crate::offline::run_gates(kernel, project_folder, target, &compiled, world, None).await {
        Ok(gates) if gates.passed => promote_next(target, &compiled).unwrap_or(ready),
        Ok(_) => tests(
            "to see which offline tests did not pass, and correct them before a publish".to_owned(),
        ),
        Err(_) => tests("to see whether the publish gates pass".to_owned()),
    }
}

/// The version a publish of `target` is stored as: the one given, or the
/// next whole number. A version the target has already is refused.
///
/// # Errors
///
/// `input`/`schema_invalid` for a label that is not a version, or one the
/// target already has; the store's failures.
pub(crate) async fn chosen_version(
    kernel: &Kernel,
    workspace: a2m_core::WorkspaceId,
    target: &TargetName,
    given: Option<&str>,
) -> Result<Version, Failure> {
    let listed = target.clone();
    let earlier = kernel
        .store()?
        .read(move |tx| tx.versions().list(&workspace, &listed))
        .await?;
    let version = match given {
        Some(text) => text.parse::<Version>().map_err(|_| {
            codes::input(
                codes::SCHEMA_INVALID,
                "The version is not three numbers joined by dots.",
                "Write the version as, for example, `2.0.0`.",
            )
        })?,
        None => {
            let next = earlier.len() + 1;
            Version::new(u32::try_from(next).unwrap_or(u32::MAX), 0, 0)
        }
    };
    // The drafts' own version is never published: a compiled target that
    // carries it is a draft's, and a pack tells a draft from a published
    // compiled target by it (decisions 0270 and 0273).
    if version == DRAFT_VERSION {
        return Err(codes::input(
            codes::SCHEMA_INVALID,
            format!(
                "The version {DRAFT_VERSION} is the one every compiled target compiled from the drafts carries, so no published version takes it."
            ),
            "Publish under a version above it, for example `1.0.0`, or give no version to take the next one.",
        ));
    }
    if earlier.iter().any(|row| row.version == version) {
        return Err(codes::input(
            codes::SCHEMA_INVALID,
            format!(
                "The target `{target}` already has a version {version}. A published version is never changed."
            ),
            "Publish under the next version.",
        ));
    }
    Ok(version)
}

/// The surface `target.add` declares from an address: its kind, the key
/// its pack states the address under, and the address.
struct AddedSurface {
    kind: a2m_core::SurfaceKind,
    key: String,
    address: String,
    /// The surface's other fields, by name, each a JSON scalar written as
    /// data: what its kind's pack declares beyond the address, such as a
    /// database surface's store and port (field finding 17). The compiler
    /// checks them against the kind before anything is written.
    fields: Vec<(String, Json)>,
}

/// The surface fields `target.add` was given: each a plain name that is
/// neither the kernel's nor the address's, with a text, a number, or true
/// or false. Which names a kind takes is its pack's, and the compiler
/// refuses one it does not declare.
fn given_fields(given: Option<&Json>, key: &str) -> Result<Vec<(String, Json)>, Failure> {
    let Some(given) = given else {
        return Ok(Vec::new());
    };
    let refused = |why: String| {
        codes::input(
            codes::SCHEMA_INVALID,
            why,
            "Give `fields` as a map of the surface's field names, as its kind's pack documents them, to texts, numbers, or true or false.",
        )
    };
    let Some(map) = given.as_object() else {
        return Err(refused("`fields` is not a map.".to_owned()));
    };
    let mut fields = Vec::new();
    for (name, value) in map {
        let plain = !name.is_empty()
            && name
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_');
        if !plain {
            return Err(refused(format!(
                "The field name `{}` is not a plain name.",
                a2m_core::said_neutral(name)
            )));
        }
        if name == key || a2m_files::kernel::SURFACE_KEYS.contains(&name.as_str()) {
            return Err(refused(format!(
                "The field `{name}` is not given in `fields`: the address is `address`, the kind is `kind`, and the kernel's own fields are written in the target file."
            )));
        }
        if !matches!(value, Json::String(_) | Json::Number(_) | Json::Bool(_)) {
            return Err(refused(format!(
                "The field `{name}` holds a list, a map, or nothing; `target.add` writes texts, numbers, and true or false."
            )));
        }
        fields.push((name.clone(), value.clone()));
    }
    Ok(fields)
}

/// The surface `target.add` is asked to declare, or none. Given an address
/// and a kind, the key is the one the kind's pack registered, asked of the
/// compiler set: this crate holds no pack's knowledge (rule 8). An address
/// with no kind is never guessed from (rule 7): the refusal names the kinds
/// the registered packs offer.
fn added_surface(
    set: &a2m_files::CompilerSet,
    address: Option<&str>,
    kind: Option<&str>,
    fields: Option<&Json>,
) -> Result<Option<AddedSurface>, Failure> {
    let offered = || {
        let kinds: Vec<String> = set
            .surface_kinds()
            .map(|kind| format!("`{kind}`"))
            .collect();
        if kinds.is_empty() {
            "No enabled pack offers a kind of surface.".to_owned()
        } else {
            format!("The enabled packs offer: {}.", kinds.join(", "))
        }
    };
    match (address, kind) {
        (None, None) if fields.is_some() => Err(codes::input(
            codes::SCHEMA_INVALID,
            "Fields of a surface were given with no surface: no address and no kind.",
            "Call `target.add` again with `address` and `kind` beside `fields`.",
        )),
        (None, None) => Ok(None),
        (Some(_), None) => Err(codes::input(
            codes::SCHEMA_INVALID,
            format!(
                "An address was given with no kind of surface, and the kind is never guessed from an address. {}",
                offered()
            ),
            "Call `target.add` again with `kind`, the kind of the system at the address.",
        )),
        (None, Some(_)) => Err(codes::input(
            codes::SCHEMA_INVALID,
            "A kind of surface was given with no address for it.",
            "Call `target.add` again with `address`, where the system is reached.",
        )),
        (Some(address), Some(kind)) => {
            let named = a2m_core::SurfaceKind::new(kind)?;
            let Some(key) = set.address_key(&named) else {
                return Err(codes::input(
                    codes::SCHEMA_INVALID,
                    format!(
                        "No enabled pack registered a surface of the kind `{}`. {}",
                        a2m_core::said_neutral(kind),
                        offered()
                    ),
                    "Name one of those kinds, or enable the pack that knows the system.",
                ));
            };
            Ok(Some(AddedSurface {
                fields: given_fields(fields, key)?,
                key: key.to_owned(),
                kind: named,
                address: address.to_owned(),
            }))
        }
    }
}

/// The first target file of `target`: with one surface of the kind given,
/// at its address, and one test environment, or with no surface yet.
fn first_target_file(target: &TargetName, surface: Option<&AddedSurface>) -> String {
    let Some(surface) = surface else {
        return format!(
            "# The target `{target}`. Fill in its surfaces, then run `anything2mcp draft lint {target}`.\n\
             schema_version: 1\ntarget: {target}\ntitle: {target}\nsurfaces: {{}}\n"
        );
    };
    // A JSON string is a YAML double-quoted scalar: the address is written
    // as data, whatever it holds.
    let address = Json::String(surface.address.clone()).to_string();
    let kind = &surface.kind;
    // A JSON scalar is a YAML scalar of the same value: each field is
    // written as data.
    let fields: String = surface
        .fields
        .iter()
        .map(|(name, value)| format!("    {name}: {value}\n"))
        .collect();
    format!(
        "# The target `{target}`, with the one surface it was added with. Run `anything2mcp scout run {target}` next.\n\
         schema_version: 1\ntarget: {target}\ntitle: {target}\n\
         surfaces:\n  {kind}:\n    kind: {kind}\n    {key}: {address}\n{fields}\
         environments:\n  test:\n    kind: test\ndefault_environment: test\n",
        key = surface.key,
    )
}

/// The compiler's refusal of a first target file, its errors by name.
fn first_file_refused(problems: &Diagnostics) -> Failure {
    let named: Vec<String> = problems
        .iter()
        .filter(|problem| problem.level == a2m_files::Level::Error)
        .take(8)
        .map(|problem| format!("{} ({})", problem.code, problem.message))
        .collect();
    codes::input(
        codes::SCHEMA_INVALID,
        format!(
            "The compiler refuses the surface as given, so nothing was written: {}",
            a2m_core::said_neutral(&named.join("; "))
        ),
        "Correct the address, the kind, or the surface's own fields as its kind declares them (`fields`, or `--fields` on the command line: a JSON object of each field's name and value), and call `target.add` again.",
    )
}

handler!(
    /// `target.add`: makes the target's folder and a first file. Given an
    /// address and a kind, the file declares one surface of that kind and
    /// one test environment, compiled as a draft before it is written
    /// (decision 0199); otherwise a file to fill in.
    target_add,
    |kernel, call| {
        let target = TargetName::new(call.required("name")?)?;
        let project_folder = call.project_folder()?.to_path_buf();
        let surface = added_surface(
            kernel.providers().compiler_set(),
            call.text("address"),
            call.text("kind"),
            call.input.get("fields"),
        )?;
        let text = first_target_file(&target, surface.as_ref());
        let declared = surface.is_some();
        let (owned, named) = (kernel.clone(), target.clone());
        // Looking at the folder, compiling, and writing the file wait on
        // the disk or compute.
        off_thread(move || {
            let folder = target_folder(&project_folder, &named)?;
            let file = folder.join("target.yaml")?;
            if file.exists() {
                return Err(codes::input(
                    codes::SCHEMA_INVALID,
                    format!("The project already has a target named `{named}`."),
                    "Choose another name, or work on the target that is there.",
                ));
            }
            if declared {
                // The file is compiled as the draft it will be, with the
                // packs that are enabled: what the compiler refuses for the
                // surface is refused here, and nothing is written.
                let set = owned.providers().compiler_set();
                let path = a2m_files::RelPath::new("target.yaml")?;
                let source = a2m_files::SourceFile::read(path, text.as_bytes(), set)
                    .map_err(|problem| first_file_refused(&Diagnostics::from(problem)))?;
                let sources = a2m_files::Sources::from_files(
                    TargetRoot::new(folder.as_path()),
                    vec![source],
                );
                a2m_files::compile(&sources, set, CompileMode::Draft, DRAFT_VERSION)
                    .map_err(|problems| first_file_refused(&problems))?;
            }
            folder.create_dir_all()?;
            if matches!(
                file.entry_kind()?,
                a2m_provider::EntryKind::Folder | a2m_provider::EntryKind::Link
            ) {
                return Err(codes::input(
                    codes::PATH_NOT_ALLOWED,
                    "A folder or a link stands at the target's `target.yaml`, and a file is never written through one.",
                    "Remove it, or choose another name.",
                ));
            }
            file.write(text.as_bytes())
        })
        .await?;
        // The address given here is the first a person registers for the
        // target: a draft that reaches another is published only from the
        // page that names it (spec section 502, rule 2).
        if let Some(surface) = &surface {
            kernel
                .register_origins(
                    &target,
                    std::iter::once(crate::origins::origin_of(&surface.address)).collect(),
                )
                .await?;
        }
        let next = if declared {
            Next::new(
                "scout.run",
                json!({"target": target.as_str()}),
                "to observe the surface and draft its reads",
            )
        } else {
            Next::new(
                "draft.lint",
                json!({"target": target.as_str()}),
                "after declaring the target's surfaces",
            )
        };
        Ok(Output::ok(json!({
            "target": target.as_str(),
            "folder": format!("{TARGETS_FOLDER}/{target}"),
            "files": ["target.yaml"],
        }))
        .then(next))
    }
);

handler!(
    /// `draft.lint`: lints and compiles a target's drafts. Changes nothing.
    draft_lint,
    |kernel, call| {
        let target = TargetName::new(call.required("target")?)?;
        let compiled =
            compiled_target(kernel, &call, &target, CompileMode::Draft, DRAFT_VERSION).await?;
        // The drafts that wait in the holding folder are listed whatever
        // the lint says: whether the target is ok does not depend on them.
        let held = held_drafts(&call, &target).await?;
        let output = match compiled {
            Ok(compiled) => {
                let ready = Next::new(
                    "version.publish",
                    json!({"target": target.as_str()}),
                    "when a person has reviewed the drafts",
                );
                Output::ok(json!({
                    "target": target.as_str(),
                    "ok": true,
                    "problems": problems_json(&compiled.warnings),
                    "held": held_json(&held),
                    "held_cut": held.cut,
                    "compiled_target_hash": compiled.encoded.hash.to_string(),
                    "tools": compiled.compiled_target.tools.iter().map(|tool| tool.name.to_string()).collect::<Vec<_>>(),
                }))
                .then(publish_next(kernel, &call, &target, ready).await)
            }
            // The answer is the failure `draft.try` gives for the same
            // drafts, `lint_failed`, and it still carries every problem.
            Err(problems) => {
                let note = held_note(&held);
                let failed = Output::ok(json!({
                    "target": target.as_str(),
                    "ok": false,
                    "problems": problems_json(&problems),
                    "held": held_json(&held),
                    "held_cut": held.cut,
                }))
                .failed(Failure::from(problems));
                match note {
                    Some(note) => failed.warn(note),
                    None => failed,
                }
                .then(Next::new(
                    "draft.lint",
                    json!({"target": target.as_str()}),
                    "after correcting the files the problems name",
                ))
            }
        };
        Ok(output)
    }
);

impl Kernel {
    /// Stores a compiled target as a version of its target, makes it the
    /// current one, and writes the audit entry, in one transaction.
    pub(crate) async fn store_version(
        &self,
        encoded: EncodedCompiledTarget,
        target: TargetName,
        version: Version,
        by: PrincipalId,
        gate_evidence: Option<String>,
    ) -> Result<Digest, Failure> {
        let workspace = self.installation()?.workspace;
        let hash = encoded.hash;
        let detail = format!("version {version}, compiled target {hash}");
        let subject = target.to_string();
        self.store()?
            .write(Durability::Synced, move |tx| {
                tx.versions().put(&PublishedVersion {
                    workspace,
                    target: target.clone(),
                    version,
                    compiled_target: encoded,
                    published_by: by,
                    gate_evidence,
                })?;
                tx.versions().make_current(&workspace, &target, &hash)?;
                note_published(tx, &workspace, &target)?;
                tx.audit()
                    .append(&a2m_ledger::AuditEntry {
                        workspace,
                        actor: by.to_string(),
                        action: "version.publish".to_owned(),
                        subject,
                        detail,
                    })
                    .map(|_| ())
            })
            .await?;
        Ok(hash)
    }

    /// Stores a compiled target built by hand as the current version of its
    /// target, so that a test of a crate above this one has a published tool
    /// to call without the compiler. Never part of the product: a compiled
    /// target reaches the store of a product only through `version.publish`.
    ///
    /// # Errors
    ///
    /// The compiled target's own check and encoding failures; the store's
    /// failures.
    #[cfg(any(test, feature = "testing"))]
    pub async fn publish_compiled_target(
        &self,
        compiled_target: &a2m_core::CompiledTarget,
    ) -> Result<Digest, Failure> {
        compiled_target.check()?;
        let by = self.installation()?.user;
        self.store_version(
            compiled_target.encode()?,
            compiled_target.header.target.clone(),
            compiled_target.header.version,
            by,
            None,
        )
        .await
    }
}

handler!(
    /// `version.publish`: compiles the drafts for publishing and stores the
    /// compiled target as an immutable version. The dispatcher has asked for
    /// proof that a person is present before this runs.
    version_publish,
    |kernel, call| {
        let target = TargetName::new(call.required("target")?)?;
        let workspace = call.caller.principal.workspace;
        let version = chosen_version(kernel, workspace, &target, call.text("version")).await?;
        let compiled =
            match compiled_target(kernel, &call, &target, CompileMode::Publish, version).await? {
                Ok(compiled) => compiled,
                Err(problems) => {
                    let held = held_drafts(&call, &target).await?;
                    return Err(noting_held(
                        crate::offline::publish_refusal(&target, problems),
                        &held,
                    ));
                }
            };
        // A publish a person was shown on a page carries the hash of the
        // compiled target they read: drafts changed since are not published.
        if let Some(pinned) = call.text("compiled_target_hash")
            && pinned != compiled.encoded.hash.to_string()
        {
            return Err(codes::stale(
                codes::DRAFTS_CHANGED,
                "The drafts changed after the publish was asked for, so they compile to another compiled target than the one shown. Nothing was published.",
                "Ask for the publish again; the person reads the new compiled target on a new page.",
            ));
        }
        // A draft cannot widen the target's reach by itself (spec section
        // 502, rule 2): an address no person registered is published only
        // from the publish page, which names it, and never from a command.
        let unregistered = kernel
            .unregistered_origins(&target, &compiled.compiled_target)
            .await?;
        let from_page = call
            .presence
            .as_ref()
            .is_some_and(|proof| proof.act().is_some());
        if !unregistered.is_empty() && !from_page {
            return Err(codes::policy(
                codes::ADDRESS_UNREGISTERED,
                format!(
                    "The drafts of `{target}` reach {}, which no person registered for the target, so nothing was published.",
                    unregistered
                        .iter()
                        .map(|place| format!("`{place}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                format!(
                    "Ask for the publish on its page, with `anything2mcp publish request {target}`, read the new address it names, and press there."
                ),
            ));
        }
        // The publish gates (spec section 403): a version is published only
        // when every gate this build has passes, and its evidence is
        // stored with it (decision 0180).
        let world = crate::offline::OfflineWorld {
            workspace,
            principal: call.caller.principal.principal.id,
            vault: kernel.inner.vault.clone(),
        };
        let gates = crate::offline::run_gates(
            kernel,
            call.project_folder()?.to_path_buf(),
            &target,
            &compiled,
            world,
            None,
        )
        .await?;
        if !gates.passed {
            return Err(gates.refusal(&target));
        }
        let evidence = gates.evidence();
        let stored_evidence = String::from_utf8(
            a2m_core::CanonicalJson::encode(&evidence)?
                .as_bytes()
                .to_vec(),
        )
        .ok();
        let tools: Vec<String> = compiled
            .compiled_target
            .tools
            .iter()
            .map(|tool| tool.name.to_string())
            .collect();
        let origins = crate::origins::origins_of(&compiled.compiled_target);
        let hash = kernel
            .store_version(
                compiled.encoded,
                target.clone(),
                version,
                call.caller.principal.principal.id,
                stored_evidence,
            )
            .await?;
        // What a person published, they registered: every place it leads.
        kernel.register_origins(&target, origins).await?;
        // A watch is tied to the version its person confirmed: every watch
        // of another version pauses or ends now (decision 0262).
        kernel.watches_after_publish(&target, hash).await;
        let mut output = Output::ok(json!({
            "target": target.as_str(),
            "version": version.to_string(),
            "compiled_target_hash": hash.to_string(),
            "tools": tools,
            "current": true,
            "gates": evidence,
        }));
        for warning in compiled.warnings.iter() {
            output = output.warn(format!("{}: {}", warning.code, warning.message));
        }
        Ok(output.then(Next::new(
            "tool.search",
            json!({"target": target.as_str()}),
            "to see the published tools",
        )))
    }
);

handler!(
    /// `publish.request`: an agent asks for a target's drafts to be
    /// published. Nothing is published here: the publish is handed to the
    /// person surface as a person's act, with the compiled target it will
    /// store, and the person decides on its page (decision 0163).
    publish_request,
    |kernel, call| {
        let def = kernel
            .actions()
            .find("version.publish")
            .cloned()
            .ok_or_else(|| {
                codes::input(
                    codes::ACTION_NOT_BUILT,
                    "`version.publish` is not in this engine's registry.",
                    "Call `action.search` to see which actions are built.",
                )
            })?;
        let mut input = serde_json::Map::new();
        input.insert("target".to_owned(), json!(call.required("target")?));
        if let Some(version) = call.text("version") {
            input.insert("version".to_owned(), json!(version));
        }
        if let Some(tools) = chosen_tools(&call) {
            input.insert("tools".to_owned(), json!(tools));
        }
        let project = (call.project.clone(), call.project_refused.clone());
        let target = call.required("target")?.to_owned();
        let output = kernel
            .ask_person(&def, &call.caller, input.clone(), project)
            .await?;
        Ok(match output.exit {
            // The page waits: the same request, asked again, says what the
            // person did (package 5.6b).
            ExitClass::PersonNeeded => output.then(Next::new(
                "publish.request",
                Json::Object(input),
                "after the person has carried out the publish on its page; the same request then says what was published",
            )),
            // The person published: said as published, with its version,
            // and nothing pending.
            ExitClass::Succeeded => {
                let mut output = output;
                let version = output.result["version"].as_str().unwrap_or_default().to_owned();
                if let Json::Object(result) = &mut output.result {
                    result.insert("state".to_owned(), json!("published"));
                }
                output.warnings = vec![format!(
                    "The person published version `{version}` of `{target}` on its page; it is the current version, and nothing waits. Asking again after the approval window, or with another `version`, asks for a new publish."
                )];
                output.then(Next::new(
                    "tool.search",
                    json!({"target": target}),
                    "to see the published tools, then call one with `tool.call`",
                ))
            }
            _ => output,
        })
    }
);
