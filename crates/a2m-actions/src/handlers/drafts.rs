//! A target's draft files, read and written through the engine, and what
//! the engine can say about a target's build (decision 0164).
//!
//! Every path is a name inside the target's folder of the checked project
//! folder: a `RelPath` of `a2m-files` (no `..`, no drive, no leading `/`),
//! with no part that starts with a dot, reached through a checked path of
//! `a2m-provider` that follows a link only while it stays inside the
//! project and never writes through one. Nothing an input names leaves the
//! project.

use a2m_core::{Failure, TargetName};
use a2m_files::{
    CompileMode, DRAFT_VERSION, Diagnostics, LARGEST_TARGET_FILE, Level, RelPath, SourceFile,
    Sources, Stage, TargetRoot,
};
use a2m_provider::{CheckedPath, EntryKind};
use serde_json::{Value as Json, json};

use super::build::{TARGETS_FOLDER, off_thread, problems_json, project, target_folder};
use super::handler;
use crate::codes;
use crate::dispatch::{Next, Output};
use crate::kernel::published_targets;

/// A draft file's path inside its target's folder, checked: a `RelPath`
/// with no hidden part.
fn draft_path(text: &str) -> Result<RelPath, Failure> {
    let path = RelPath::new(text)?;
    if path.parts().any(|part| part.starts_with('.')) {
        return Err(codes::input(
            codes::PATH_NOT_ALLOWED,
            "A draft file's path has a part that starts with a dot; such names are not target files.",
            "Name the file by its path inside the target's folder, such as `operations/order_get.yaml`.",
        ));
    }
    Ok(path)
}

/// The draft file at `path` of the target's folder, as a checked path.
fn draft_file(folder: &CheckedPath, path: &RelPath) -> Result<CheckedPath, Failure> {
    folder.join(path.as_str())
}

fn no_such_file(target: &TargetName, path: &RelPath) -> Failure {
    codes::input(
        codes::NOT_FOUND,
        format!("The target `{target}` has no draft file `{path}`."),
        "Call `target.status` to see the target's files, or write the file with `draft.write`.",
    )
}

handler!(
    /// `draft.read`: one draft file of a target, as text.
    draft_read,
    |_kernel, call| {
        let target = TargetName::new(call.required("target")?)?;
        let path = draft_path(call.required("file")?)?;
        let project_folder = call.project_folder()?.to_path_buf();
        let (named, at) = (target.clone(), path.clone());
        let bytes = off_thread(move || {
            let folder = target_folder(&project_folder, &named)?;
            let file = draft_file(&folder, &at)?;
            match file.entry_kind()? {
                EntryKind::File => file.read(LARGEST_TARGET_FILE as u64),
                EntryKind::Absent => Err(no_such_file(&named, &at)),
                _ => Err(codes::input(
                    codes::PATH_NOT_ALLOWED,
                    format!("`{at}` is not a file: a link, a folder, or a device is not read."),
                    "Name a file of the target's folder.",
                )),
            }
        })
        .await?;
        let content = String::from_utf8(bytes).map_err(|_| {
            codes::input(
                codes::SCHEMA_INVALID,
                format!("`{path}` is not UTF-8 text, so it is not read here."),
                "Read a target file or a fixture's text file.",
            )
        })?;
        Ok(Output::ok(json!({
            "target": target.as_str(),
            "file": path.as_str(),
            "size_bytes": content.len(),
            "digest": a2m_core::Digest::of(content.as_bytes()).to_string(),
            "content": content,
        })))
    }
);

/// Whether a problem is the edited file's own: it does not read, or does
/// not fit its kind's schema. Such a file is not written.
fn its_own(problem: &a2m_files::Diagnostic, path: &RelPath) -> bool {
    problem.level == Level::Error
        && problem.at.file == *path
        && matches!(problem.stage, Stage::Parse | Stage::Validate)
}

handler!(
    /// `draft.write`: one draft file, validated and linted before it is
    /// written. A file that does not read, or does not fit its kind's
    /// schema, is not written; a file that reads is written, and the lint
    /// of the whole target is returned with it, because a target being
    /// built has problems in other files until it is done.
    draft_write,
    |kernel, call| {
        let target = TargetName::new(call.required("target")?)?;
        let path = draft_path(call.required("file")?)?;
        let content = call.required("content")?.to_owned();
        if content.len() > LARGEST_TARGET_FILE {
            return Err(codes::input(
                codes::SCHEMA_INVALID,
                format!(
                    "The file is larger than the {LARGEST_TARGET_FILE} bytes a target file may hold."
                ),
                "Split the file.",
            ));
        }
        let project_folder = call.project_folder()?.to_path_buf();
        let (kernel, named, at) = (kernel.clone(), target.clone(), path.clone());
        let written = off_thread(move || {
            let set = kernel.providers().compiler_set();
            let folder = target_folder(&project_folder, &named)?;
            if !folder.is_dir() {
                return Err(codes::input(
                    codes::NOT_FOUND,
                    format!("The project has no target named `{named}`."),
                    "Add it with `target.add` first.",
                ));
            }
            let edited = match SourceFile::read(at.clone(), content.as_bytes(), set) {
                Ok(edited) => edited,
                Err(problem) => return Ok(Err(Diagnostics::from(problem))),
            };
            let root = TargetRoot::new(folder.as_path());
            let previous = a2m_files::load(&root, set)
                .unwrap_or_else(|_| Sources::from_files(root.clone(), Vec::new()));
            let problems = a2m_files::lint_one(&previous, edited.clone(), set);
            if problems.iter().any(|problem| its_own(problem, &at)) {
                return Ok(Err(problems));
            }
            let file = draft_file(&folder, &at)?;
            if matches!(file.entry_kind()?, EntryKind::Folder | EntryKind::Link) {
                return Err(codes::input(
                    codes::PATH_NOT_ALLOWED,
                    format!("A folder or a link stands at `{at}`, and a file is never written through one."),
                    "Name another file.",
                ));
            }
            if let Some((parent, _)) = at.as_str().rsplit_once('/') {
                folder.join(parent)?.create_dir_all()?;
            }
            file.write(content.as_bytes())?;
            Ok(Ok((edited.digest, problems)))
        })
        .await?;
        let output = match written {
            Ok((digest, problems)) => Output::ok(json!({
                "target": target.as_str(),
                "file": path.as_str(),
                "written": true,
                "digest": digest.to_string(),
                "ok": !problems.has_errors(),
                "problems": problems_json(&problems),
            }))
            .then(Next::new(
                "draft.lint",
                json!({"target": target.as_str()}),
                "after the last file of the change is written",
            )),
            Err(problems) => Output::ok(json!({
                "target": target.as_str(),
                "file": path.as_str(),
                "written": false,
                "ok": false,
                "problems": problems_json(&problems),
            }))
            .then(Next::new(
                "draft.write",
                json!({"target": target.as_str(), "file": path.as_str()}),
                "after correcting what the problems name; nothing was written",
            )),
        };
        Ok(output)
    }
);

/// The names of the targets in the project folder: each folder under
/// `targets/` that holds a `target.yaml`. Looks at the disk.
fn project_targets(project_folder: &std::path::Path) -> Result<Vec<String>, Failure> {
    let targets = project(project_folder)?.join(TARGETS_FOLDER)?;
    if !targets.is_dir() {
        return Ok(Vec::new());
    }
    let (entries, _) = targets.list()?;
    let mut names = Vec::new();
    for entry in entries {
        let Some(name) = entry.relative().rsplit('/').next() else {
            continue;
        };
        if TargetName::new(name).is_err() || !entry.is_dir() {
            continue;
        }
        if matches!(entry.join("target.yaml")?.entry_kind()?, EntryKind::File) {
            names.push(name.to_owned());
        }
    }
    Ok(names)
}

handler!(
    /// `target.list`: the targets in the project folder, and those with a
    /// published version, with the versions of each.
    target_list,
    |kernel, call| {
        let workspace = call.caller.principal.workspace;
        let published = kernel
            .store()?
            .read(move |tx| {
                let mut out = Vec::new();
                for target in published_targets(tx, &workspace)? {
                    let versions = tx.versions().list(&workspace, &target)?;
                    out.push((target.to_string(), versions));
                }
                Ok(out)
            })
            .await?;
        // A call made in a project folder lists its targets too; one made
        // elsewhere lists what was published.
        let in_project = match call.project_folder() {
            Ok(folder) => {
                let folder = folder.to_path_buf();
                off_thread(move || project_targets(&folder)).await?
            }
            Err(_) => Vec::new(),
        };
        let mut names: Vec<String> = published.iter().map(|(name, _)| name.clone()).collect();
        names.extend(in_project.iter().cloned());
        names.sort();
        names.dedup();
        let listed: Vec<Json> = names
            .iter()
            .map(|name| {
                let versions = published
                    .iter()
                    .find(|(published, _)| published == name)
                    .map(|(_, versions)| {
                        versions
                            .iter()
                            .map(|row| {
                                json!({
                                    "version": row.version.to_string(),
                                    "plan_hash": row.plan_hash.to_string(),
                                    "current": row.current,
                                })
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                json!({
                    "target": name,
                    "in_project": in_project.contains(name),
                    "versions": versions,
                })
            })
            .collect();
        Ok(Output::ok(json!({"targets": listed})))
    }
);

/// The files of a target's folder, by path, as the compiler reads them.
fn file_list(sources: &Sources) -> Vec<Json> {
    sources
        .files
        .iter()
        .map(|file| json!({"file": file.path.as_str(), "kind": file.kind.as_str()}))
        .collect()
}

handler!(
    /// `target.status`: where the target's build stands, from what the
    /// engine can see itself, and the next action. The states between
    /// added and drafted that need a system, signed in and explored, and
    /// tried and tested, are not kept yet and are named as unknown.
    target_status,
    |kernel, call| {
        let target = TargetName::new(call.required("target")?)?;
        let workspace = call.caller.principal.workspace;
        let listed = target.clone();
        let versions = kernel
            .store()?
            .read(move |tx| tx.versions().list(&workspace, &listed))
            .await?;
        let current = versions.iter().find(|row| row.current);
        let project_folder = call.project_folder()?.to_path_buf();
        let (worker, named) = (kernel.clone(), target.clone());
        let looked = off_thread(move || {
            let folder = target_folder(&project_folder, &named)?;
            if !matches!(folder.join("target.yaml")?.entry_kind()?, EntryKind::File) {
                return Ok(None);
            }
            let set = worker.providers().compiler_set();
            let root = TargetRoot::new(folder.as_path());
            let loaded = a2m_files::load(&root, set);
            let files = loaded.as_ref().map(file_list).unwrap_or_default();
            let compiled = loaded.and_then(|sources| {
                a2m_files::compile(&sources, set, CompileMode::Draft, DRAFT_VERSION)
            });
            Ok(Some((files, compiled.map(|compiled| compiled.warnings))))
        })
        .await?;
        let Some((files, compiled)) = looked else {
            return Err(codes::input(
                codes::NOT_FOUND,
                format!("The project has no target named `{target}`."),
                "Add it with `target.add`.",
            ));
        };
        let (compiles, problems) = match &compiled {
            Ok(warnings) => (true, problems_json(warnings)),
            Err(problems) => (false, problems_json(problems)),
        };
        let state = match (compiles, current) {
            (_, Some(_)) => "published",
            (true, None) => "drafted",
            (false, None) => "added",
        };
        let next = match (compiles, current) {
            (false, _) => Next::new(
                "draft.lint",
                json!({"target": target.as_str()}),
                "after correcting the files the problems name",
            ),
            (true, None) => Next::new(
                "publish.request",
                json!({"target": target.as_str()}),
                "when the drafts are ready for a person's review",
            ),
            (true, Some(_)) => Next::new(
                "tool.search",
                json!({"target": target.as_str()}),
                "to see the published tools",
            ),
        };
        Ok(Output::ok(json!({
            "target": target.as_str(),
            "state": state,
            "compiles": compiles,
            "problems": problems,
            "files": files,
            "current_version": current.map(|row| row.version.to_string()),
            "versions": versions.len(),
            "unknown": ["signed_in", "explored", "tried", "tested"],
        }))
        .then(next))
    }
);

handler!(
    /// `test.run`: the publish gates on a target's drafts, as far as the
    /// engine can run them. The schema and lint gate is the compiler's; an
    /// offline test needs a replay of its recorded fixture through a
    /// driver, which neither the engine nor the provider contract offers
    /// yet, so each is listed as not run, and the result says that the
    /// gates are not complete. Nothing is called and nothing is written.
    test_run,
    |kernel, call| {
        let target = TargetName::new(call.required("target")?)?;
        let project_folder = call.project_folder()?.to_path_buf();
        let compiled = super::build::compile_in(
            kernel,
            project_folder,
            &target,
            CompileMode::Publish,
            DRAFT_VERSION,
        )
        .await?;
        let (lint, tests) = match &compiled {
            Ok(compiled) => (
                json!({"gate": "schema_and_lint", "state": "passed", "problems": problems_json(&compiled.warnings)}),
                compiled
                    .plan
                    .tests
                    .iter()
                    .map(|test| {
                        let tool = compiled
                            .plan
                            .tools
                            .get(test.tool.get())
                            .map(|tool| tool.name.to_string())
                            .unwrap_or_default();
                        json!({
                            "test": test.name,
                            "tool": tool,
                            "kind": test.kind.to_string(),
                            "fixture": test.fixture,
                            "state": "not_run",
                            "reason": "No replay of a recorded fixture through a driver exists yet in the engine or the provider contract.",
                        })
                    })
                    .collect::<Vec<_>>(),
            ),
            Err(problems) => (
                json!({"gate": "schema_and_lint", "state": "failed", "problems": problems_json(problems)}),
                Vec::new(),
            ),
        };
        let passed_lint = compiled.is_ok();
        let output = Output::ok(json!({
            "target": target.as_str(),
            "passed": false,
            "complete": false,
            "gates": [
                lint,
                {"gate": "offline_replay", "state": "not_run", "tests": tests},
            ],
            "not_run": ["offline_replay", "contract", "determinism", "binding_equivalence", "write_round_trip", "idempotency", "resume", "fault_injection", "performance"],
        }))
        .warn("Only the schema and lint gate runs today; the result is never a pass.");
        Ok(output.then(if passed_lint {
            Next::new(
                "publish.request",
                json!({"target": target.as_str()}),
                "when a person should review the drafts",
            )
        } else {
            Next::new(
                "draft.lint",
                json!({"target": target.as_str()}),
                "after correcting the files the problems name",
            )
        }))
    }
);
