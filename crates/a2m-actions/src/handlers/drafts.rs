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

use super::build::{
    TARGETS_FOLDER, held_drafts, held_note, off_thread, problems_json, project, target_folder,
};
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

/// Refuses a path that passes through a link below the target's folder
/// (audit finding DD-1): a draft is written or removed only in the target's
/// own folders, never in a folder a link leads to, even one inside the
/// project. A part not there yet is no link.
fn no_link_on_the_way(folder: &CheckedPath, path: &RelPath) -> Result<(), Failure> {
    let parts: Vec<&str> = path.parts().collect();
    let mut at = String::new();
    for part in parts.iter().take(parts.len().saturating_sub(1)) {
        if !at.is_empty() {
            at.push('/');
        }
        at.push_str(part);
        match folder.join(&at)?.entry_kind()? {
            EntryKind::Folder | EntryKind::Absent => {}
            _ => {
                return Err(codes::input(
                    codes::PATH_NOT_ALLOWED,
                    format!(
                        "`{at}` of the target is a link or not a folder, and a draft is never written or removed through one."
                    ),
                    "Make it a folder of the target's own.",
                ));
            }
        }
    }
    Ok(())
}

/// The path a draft of the holding folder would have outside it, which
/// names its file kind: `operations/x.yaml` for `held/operations/x.yaml`.
/// `None` for a path that is not under the holding folder.
fn outside_held(path: &RelPath) -> Option<RelPath> {
    let inner = path.as_str().strip_prefix("held/")?;
    RelPath::new(inner).ok()
}

fn not_a_held_draft(path: &RelPath) -> Failure {
    codes::input(
        codes::PATH_NOT_ALLOWED,
        format!("`{path}` is not a draft in a folder of the holding folder."),
        "Name a held draft by its path, such as `held/operations/stock_get.yaml`.",
    )
}

/// The fix of a refusal of a name, with how a list is written when the
/// name holds a comma or reads as a list, and so was most likely meant as
/// several (package 5.6b).
fn fix_for_name(path: &RelPath, fix: &str) -> String {
    let text = path.as_str();
    if text.contains(',') || text.starts_with('[') {
        format!(
            "If several files were meant, name each one: on the command line, repeat the flag for each (`--files a --files b`) or give one JSON array of text; in an action's input, `files` is a JSON array. {fix}"
        )
    } else {
        fix.to_owned()
    }
}

fn no_such_file(target: &TargetName, path: &RelPath) -> Failure {
    codes::input(
        codes::NOT_FOUND,
        format!("The target `{target}` has no draft file `{path}`."),
        fix_for_name(
            path,
            "Call `target.status` to see the target's files, or write the file with `draft.write`.",
        ),
    )
}

handler!(
    /// `draft.read`: one draft file of a target, as text.
    draft_read,
    |_kernel, call| {
        let target = TargetName::new(call.required("target")?)?;
        let path = draft_path(call.required("file")?)?;
        if outside_held(&path).is_some_and(|inner| inner.folder().is_none()) {
            return Err(not_a_held_draft(&path));
        }
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

/// The refusal of a written file that makes a binding verified, or keeps
/// one verified while changing it (audit finding PM-2): only
/// `binding.promote` makes a binding verified, on its evidence. Each binding
/// the new text holds as `verified` must be, at the same place of the same
/// operation, a binding the target's files on disk hold as `verified`, of
/// the same mode, written the same. `None` where the text may be written.
fn verified_by_hand(previous: &Sources, edited: &SourceFile) -> Option<Failure> {
    if edited.kind.as_str() != "operation" {
        return None;
    }
    let before = |operation: &str| {
        previous
            .of_kind("operation")
            .flat_map(|file| file.docs.iter())
            .find(|doc| doc.text("operation") == Some(operation))
    };
    for doc in &edited.docs {
        let Some(operation) = doc.text("operation") else {
            continue;
        };
        let bindings = doc.get("bindings").map(a2m_files::Located::items);
        for (position, binding) in bindings.unwrap_or_default().iter().enumerate() {
            if binding.text("status") != Some("verified") {
                continue;
            }
            let earlier = before(operation)
                .and_then(|earlier| earlier.get("bindings"))
                .and_then(|bindings| bindings.items().get(position).cloned());
            let kept = earlier.as_ref().is_some_and(|earlier| {
                earlier.text("status") == Some("verified") && earlier.to_json() == binding.to_json()
            });
            if kept {
                continue;
            }
            let (message, next) = if earlier
                .as_ref()
                .is_some_and(|earlier| earlier.text("status") == Some("verified"))
            {
                (
                    format!(
                        "The binding {} of `{}` is verified, and this text changes it while leaving it `verified`: a changed binding has not earned what the verified one did.",
                        position + 1,
                        codes::shown(operation)
                    ),
                    "Write the changed binding with `status: draft`, test it, and make it verified with `binding.promote`.",
                )
            } else {
                (
                    format!(
                        "The binding {} of `{}` is written as `verified`, and the target's files do not hold it so: a binding is made verified only by `binding.promote`, on its evidence.",
                        position + 1,
                        codes::shown(operation)
                    ),
                    "Write it with `status: draft`, test it and try it, then call `binding.promote`.",
                )
            };
            return Some(codes::input(codes::BINDING_NOT_PROMOTABLE, message, next));
        }
    }
    None
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
            // A draft in the holding folder is read as the file it would be
            // outside it, which names its kind (decision 0298). It waits on
            // an open question, so it is not linted with the target, and
            // nothing it holds reaches the compiler.
            let held = outside_held(&at);
            if let Some(inner) = &held
                && inner.folder().is_none()
            {
                return Err(not_a_held_draft(&at));
            }
            let edited = match SourceFile::read(
                held.clone().unwrap_or_else(|| at.clone()),
                content.as_bytes(),
                set,
            ) {
                Ok(edited) => edited,
                Err(problem) => return Ok(Err(Diagnostics::from(problem))),
            };
            if held.is_some() {
                no_link_on_the_way(&folder, &at)?;
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
                return Ok(Ok((edited.digest, Diagnostics::new())));
            }
            let root = TargetRoot::new(folder.as_path());
            let loaded = a2m_files::load(&root, set).ok();
            let previous = loaded
                .clone()
                .unwrap_or_else(|| Sources::from_files(root.clone(), Vec::new()));
            // What the disk holds of the file now, where the target as a
            // whole does not load.
            let on_disk = || {
                let file = draft_file(&folder, &at).ok()?;
                let bytes = file.read(LARGEST_TARGET_FILE as u64).ok()?;
                let read = SourceFile::read(at.clone(), &bytes, set).ok()?;
                Some(Sources::from_files(root.clone(), vec![read]))
            };
            let before = match loaded {
                Some(loaded) => loaded,
                None => on_disk().unwrap_or_else(|| previous.clone()),
            };
            if let Some(refused) = verified_by_hand(&before, &edited) {
                return Err(refused);
            }
            let problems = a2m_files::lint_one(&previous, edited.clone(), set);
            if problems.iter().any(|problem| its_own(problem, &at)) {
                return Ok(Err(problems));
            }
            no_link_on_the_way(&folder, &at)?;
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
        let failed = match &written {
            Ok((_, problems)) | Err(problems) => problems.has_errors(),
        };
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
        // A name not found may be defined by a held draft: said once, and
        // the held files are looked at only when the compile has failed.
        let output = match failed
            .then_some(())
            .and(held_note(&held_drafts(&call, &target).await?))
        {
            Some(note) => output.warn(note),
            None => output,
        };
        Ok(output)
    }
);

handler!(
    /// `draft.delete`: one draft file of a target removed from the project
    /// (decision 0189). Only a file the compiler would read as one of the
    /// target's drafts, in one of its folders, is removed: never the target's
    /// own top-level files, a name no file kind claims, a link, a folder, or
    /// a device. A published version is untouched: its sources are held in
    /// the store, beside its compiled target, and no path reaches them. The
    /// lint of what is left comes back.
    draft_delete,
    |kernel, call| {
        let target = TargetName::new(call.required("target")?)?;
        // One file by `file`, or several by `files` (decision 0219): the
        // same checks for each, every one checked before any is removed,
        // and all removed or none.
        let named: Vec<String> = match (call.text("file"), call.input.get("files")) {
            (Some(one), None) => vec![one.to_owned()],
            (None, Some(Json::Array(many))) if !many.is_empty() => many
                .iter()
                .map(|file| file.as_str().map(str::to_owned))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| {
                    codes::input(
                        codes::SCHEMA_INVALID,
                        "`files` holds a value that is not a path.",
                        "Give each draft's path inside the target's folder as text.",
                    )
                })?,
            _ => {
                return Err(codes::input(
                    codes::SCHEMA_INVALID,
                    "`draft.delete` takes one draft by `file`, or several by `files`, and not both.",
                    "Name the drafts to remove, by their paths inside the target's folder.",
                ));
            }
        };
        let mut paths = Vec::with_capacity(named.len());
        for file in &named {
            let path = draft_path(file)?;
            if paths.contains(&path) {
                return Err(codes::input(
                    codes::SCHEMA_INVALID,
                    format!("`{path}` is named twice."),
                    "Name each draft once.",
                ));
            }
            paths.push(path);
        }
        let project_folder = call.project_folder()?.to_path_buf();
        let (kernel, named, all) = (kernel.clone(), target.clone(), paths.clone());
        let (removed, problems) = off_thread(move || {
            let set = kernel.providers().compiler_set();
            let folder = target_folder(&project_folder, &named)?;
            if !folder.is_dir() {
                return Err(codes::input(
                    codes::NOT_FOUND,
                    format!("The project has no target named `{named}`."),
                    "Call `target.list` to see the project's targets.",
                ));
            }
            let mut checked = Vec::with_capacity(all.len());
            for at in &all {
                if outside_held(at).is_some_and(|inner| inner.folder().is_none()) {
                    return Err(not_a_held_draft(at));
                }
                if at.folder().is_none() {
                    return Err(codes::input(
                        codes::PATH_NOT_ALLOWED,
                        format!("`{at}` is one of the target's own files, not a draft in one of its folders, and is not removed."),
                        fix_for_name(at, "Name a draft inside one of the target's folders, such as `operations/stock_get.yaml`."),
                    ));
                }
                no_link_on_the_way(&folder, at)?;
                let file = draft_file(&folder, at)?;
                match file.entry_kind()? {
                    EntryKind::File => {}
                    EntryKind::Absent => return Err(no_such_file(&named, at)),
                    _ => {
                        return Err(codes::input(
                            codes::PATH_NOT_ALLOWED,
                            format!("`{at}` is not a regular file: a link, a folder, or a device is never removed."),
                            "Name a file of the target's folder.",
                        ));
                    }
                }
                let bytes = file.read(LARGEST_TARGET_FILE as u64 + 1)?;
                // A held draft is judged as the file it would be outside
                // the holding folder (decision 0298).
                let held = outside_held(at);
                if held.as_ref().is_some_and(|inner| inner.folder().is_none()) {
                    return Err(not_a_held_draft(at));
                }
                if let Err(problem) =
                    SourceFile::read(held.unwrap_or_else(|| at.clone()), &bytes, set)
                    && problem.code.as_str() == a2m_files::codes::UNKNOWN_FILE
                {
                    return Err(codes::input(
                        codes::PATH_NOT_ALLOWED,
                        format!("`{at}` is not in a folder of the target's drafts: no file kind claims it, so it is not removed."),
                        "Name a draft the compiler reads; `target.status` lists them.",
                    ));
                }
                checked.push((at.clone(), file, bytes));
            }
            // Every file passed its checks: each is removed. A removal that
            // fails puts back the ones removed before it.
            for (done, (at, file, _)) in checked.iter().enumerate() {
                if let Err(failure) = file.remove_file() {
                    for (_, earlier, bytes) in &checked[..done] {
                        let _ = earlier.write(bytes);
                    }
                    return Err(codes::busy(
                        failure.code().clone(),
                        format!("`{at}` could not be removed, so no draft was: {}", failure.message()),
                        failure.next_step(),
                    ));
                }
            }
            let root = TargetRoot::new(folder.as_path());
            let problems = match a2m_files::load(&root, set) {
                Ok(sources) => match a2m_files::compile(&sources, set, CompileMode::Draft, DRAFT_VERSION) {
                    Ok(compiled) => compiled.warnings,
                    Err(problems) => problems,
                },
                Err(problems) => problems,
            };
            let removed: Vec<(String, a2m_core::Digest)> = checked
                .iter()
                .map(|(at, _, bytes)| (at.as_str().to_owned(), a2m_core::Digest::of(bytes)))
                .collect();
            Ok((removed, problems))
        })
        .await?;
        let mut shown = json!({
            "target": target.as_str(),
            "files": removed
                .iter()
                .map(|(file, digest)| json!({"file": file, "digest": digest.to_string()}))
                .collect::<Vec<_>>(),
            "removed": true,
            "ok": !problems.has_errors(),
            "problems": problems_json(&problems),
        });
        // One file named by `file` is answered as before.
        if let ([(file, digest)], Some(_)) = (removed.as_slice(), call.text("file")) {
            shown["file"] = json!(file);
            shown["digest"] = json!(digest.to_string());
        }
        let mut output = Output::ok(shown);
        if problems.has_errors()
            && let Some(note) = held_note(&held_drafts(&call, &target).await?)
        {
            output = output.warn(note);
        }
        Ok(output.then(Next::new(
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
                                    "compiled_target_hash": row.compiled_target_hash.to_string(),
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
            (true, None) => {
                super::build::publish_next(
                    kernel,
                    &call,
                    &target,
                    Next::new(
                        "publish.request",
                        json!({"target": target.as_str()}),
                        "when the drafts are ready for a person's review",
                    ),
                )
                .await
            }
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
                // The gates did not pass: the action failed, exit code 1,
                // with the refusal a publish would give (decision 0219).
                let refusal = crate::offline::publish_refusal(&target, problems.clone());
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
                .failed(refusal)
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
            crate::offline::run_gates(kernel, project_folder, &target, &compiled, world, None)
                .await?;
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
        // Gates that did not pass are a failed action, exit code 1, with
        // the refusal a publish would give; never "a person is needed"
        // (decision 0219).
        let output = if all_passed {
            output
        } else {
            output.failed(gates.refusal(&target))
        };
        Ok(output.then(if all_passed {
            // A draft binding that would act is promoted first: the
            // publish request comes after it (package 5.6b).
            super::build::promote_next(&target, &compiled).unwrap_or_else(|| {
                Next::new(
                    "publish.request",
                    json!({"target": target.as_str()}),
                    "when a person should review the drafts",
                )
            })
        } else {
            Next::new(
                "draft.read",
                json!({"target": target.as_str()}),
                "to correct the test, its fixture, or the binding a failed test names",
            )
        }))
    }
);
