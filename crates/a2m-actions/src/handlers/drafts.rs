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

handler!(
    /// `draft.delete`: one draft file of a target removed from the project
    /// (decision 0189). Only a file the compiler would read as one of the
    /// target's drafts, in one of its folders, is removed: never the
    /// target's own top-level files, a name no file kind claims, a link, a
    /// folder, or a device. A published version is untouched: its sources
    /// are held in the store, beside its plan, and no path reaches them. The
    /// lint of what is left comes back.
    draft_delete,
    |kernel, call| {
        let target = TargetName::new(call.required("target")?)?;
        let path = draft_path(call.required("file")?)?;
        let project_folder = call.project_folder()?.to_path_buf();
        let (kernel, named, at) = (kernel.clone(), target.clone(), path.clone());
        let (digest, problems) = off_thread(move || {
            let set = kernel.providers().compiler_set();
            let folder = target_folder(&project_folder, &named)?;
            if !folder.is_dir() {
                return Err(codes::input(
                    codes::NOT_FOUND,
                    format!("The project has no target named `{named}`."),
                    "Call `target.list` to see the project's targets.",
                ));
            }
            if at.folder().is_none() {
                return Err(codes::input(
                    codes::PATH_NOT_ALLOWED,
                    format!("`{at}` is one of the target's own files, not a draft in one of its folders, and is not removed."),
                    "Name a draft inside one of the target's folders, such as `operations/stock_get.yaml`.",
                ));
            }
            let file = draft_file(&folder, &at)?;
            match file.entry_kind()? {
                EntryKind::File => {}
                EntryKind::Absent => return Err(no_such_file(&named, &at)),
                _ => {
                    return Err(codes::input(
                        codes::PATH_NOT_ALLOWED,
                        format!("`{at}` is not a regular file: a link, a folder, or a device is never removed."),
                        "Name a file of the target's folder.",
                    ));
                }
            }
            let bytes = file.read(LARGEST_TARGET_FILE as u64 + 1)?;
            if let Err(problem) = SourceFile::read(at.clone(), &bytes, set)
                && problem.code.as_str() == a2m_files::codes::UNKNOWN_FILE
            {
                return Err(codes::input(
                    codes::PATH_NOT_ALLOWED,
                    format!("`{at}` is not in a folder of the target's drafts: no file kind claims it, so it is not removed."),
                    "Name a draft the compiler reads; `target.status` lists them.",
                ));
            }
            file.remove_file()?;
            let root = TargetRoot::new(folder.as_path());
            let problems = match a2m_files::load(&root, set) {
                Ok(sources) => match a2m_files::compile(&sources, set, CompileMode::Draft, DRAFT_VERSION) {
                    Ok(compiled) => compiled.warnings,
                    Err(problems) => problems,
                },
                Err(problems) => problems,
            };
            Ok((a2m_core::Digest::of(&bytes), problems))
        })
        .await?;
        Ok(Output::ok(json!({
            "target": target.as_str(),
            "file": path.as_str(),
            "removed": true,
            "digest": digest.to_string(),
            "ok": !problems.has_errors(),
            "problems": problems_json(&problems),
        }))
        .then(Next::new(
            "test.run",
            json!({"target": target.as_str()}),
            "to run the gates on the drafts that are left",
        )))
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
    /// engine can run them. The schema and lint gate is the compiler's.
    /// Each offline test runs its tool through the engine against a replay
    /// of its fixture's recorded answers, with no system touched (decision
    /// 0177); a test of a write, and a live test, are listed as not run.
    /// Nothing is called and nothing is written.
    test_run,
    |kernel, call| {
        let target = TargetName::new(call.required("target")?)?;
        let project_folder = call.project_folder()?.to_path_buf();
        let compiled = super::build::compile_in(
            kernel,
            project_folder.clone(),
            &target,
            CompileMode::Publish,
            DRAFT_VERSION,
        )
        .await?;
        let compiled = match compiled {
            Ok(compiled) => compiled,
            Err(problems) => {
                return Ok(Output::ok(json!({
                    "target": target.as_str(),
                    "passed": false,
                    "complete": false,
                    "gates": [
                        {"gate": "schema_and_lint", "state": "failed", "problems": problems_json(&problems)},
                        {"gate": "offline_replay", "state": "not_run", "tests": []},
                    ],
                    // An operation with no offline test, with both ways on:
                    // a test, or the draft removed (decision 0189).
                    "missing": crate::offline::without_test(&target, &problems),
                    "not_run": crate::offline::gates_not_built(),
                }))
                .then(Next::new(
                    "draft.lint",
                    json!({"target": target.as_str()}),
                    "after correcting the files the problems name",
                )));
            }
        };
        let world = crate::offline::OfflineWorld {
            workspace: call.caller.principal.workspace,
            principal: call.caller.principal.principal.id,
            vault: kernel.inner.vault.clone(),
        };
        let gates =
            crate::offline::run_gates(kernel, project_folder, &target, &compiled, world, None).await?;
        let all_passed = gates.passed;
        let output = Output::ok(json!({
            "target": target.as_str(),
            // Every gate that ran passed, and every offline test ran and
            // passed: what a publish needs. The gates listed under
            // `not_run` are not built, so the run is never complete.
            "passed": all_passed,
            "complete": false,
            "gates": gates.gates,
            "missing": gates.missing,
            "not_run": crate::offline::gates_not_built(),
        }))
        .warn("The schema and lint gate and the offline tests run; the other gates are not built, so the result is never complete.");
        Ok(output.then(if all_passed {
            Next::new(
                "publish.request",
                json!({"target": target.as_str()}),
                "when a person should review the drafts",
            )
        } else {
            Next::new(
                "draft.read",
                json!({"target": target.as_str()}),
                "to correct the test, its fixture, or the binding a failed test names",
            )
        }))
    }
);
