//! Building a target: its draft files in the project folder, and the
//! version a person publishes from them.
//!
//! This crate is the only caller of the compiler. A plan is produced here
//! and stored by `version.publish`; the engine receives it through
//! `Control` and never sees the compiler.

use a2m_core::{Digest, EncodedPlan, Failure, PrincipalId, TargetName, Version};
use a2m_files::{CompileMode, Compiled, DRAFT_VERSION, Diagnostics, TargetRoot};
use a2m_ledger::{Durability, PublishedVersion};
use a2m_provider::{Access, CheckedPath, RootName, Roots};
use serde_json::{Value as Json, json};
use std::path::Path;

use super::handler;
use crate::codes;
use crate::dispatch::{Call, Next, Output};
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
) -> Result<Compiled, Diagnostics> {
    let set = kernel.providers().compiler_set();
    let sources = a2m_files::load(&TargetRoot::new(folder.as_path()), set)?;
    a2m_files::compile(&sources, set, mode, version)
}

/// Compiles what of a target's drafts can compile, for observing and
/// trying them: every file but those `held_back` names and those that hold
/// a drafter's open question, which keep their whole kind from compiling
/// (decision 0180). Gives the plan and the files left out, in order. Never
/// used to publish: a publish compiles every file.
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
    compile_in(kernel, project_folder, target, mode, version).await
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
    let (kernel, target) = (kernel.clone(), target.clone());
    off_thread(move || {
        let folder = target_folder(&project_folder, &target)?;
        Ok(compile_target(&kernel, &folder, mode, version))
    })
    .await
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

handler!(
    /// `target.add`: makes the target's folder and a first file to fill in.
    target_add,
    |_kernel, call| {
        let target = TargetName::new(call.required("name")?)?;
        let project_folder = call.project_folder()?.to_path_buf();
        let mut text = format!(
            "# The target `{target}`. Fill in its surfaces, then run `anything2mcp draft lint {target}`.\n\
             schema_version: 1\ntarget: {target}\ntitle: {target}\n"
        );
        if let Some(address) = call.text("address") {
            // Kept as a note for a person to review; never as a surface.
            let shown: String = address
                .chars()
                .filter(|c| !c.is_control())
                .take(200)
                .collect();
            text.push_str(&format!(
                "# address given when the target was added: {shown}\n"
            ));
        }
        text.push_str("surfaces: {}\n");
        let named = target.clone();
        // Looking at the folder and writing the file wait on the disk.
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
            folder.create_dir_all()?;
            file.write(text.as_bytes())
        })
        .await?;
        Ok(Output::ok(json!({
            "target": target.as_str(),
            "folder": format!("{TARGETS_FOLDER}/{target}"),
            "files": ["target.yaml"],
        }))
        .then(Next::new(
            "draft.lint",
            json!({"target": target.as_str()}),
            "after declaring the target's surfaces",
        )))
    }
);

handler!(
    /// `draft.lint`: lints and compiles a target's drafts. Changes nothing.
    draft_lint,
    |kernel, call| {
        let target = TargetName::new(call.required("target")?)?;
        let compiled =
            compiled_target(kernel, &call, &target, CompileMode::Draft, DRAFT_VERSION).await?;
        let output = match compiled {
            Ok(compiled) => Output::ok(json!({
                "target": target.as_str(),
                "ok": true,
                "problems": problems_json(&compiled.warnings),
                "plan_hash": compiled.encoded.hash.to_string(),
                "tools": compiled.plan.tools.iter().map(|tool| tool.name.to_string()).collect::<Vec<_>>(),
            }))
            .then(Next::new(
                "version.publish",
                json!({"target": target.as_str()}),
                "when a person has reviewed the drafts",
            )),
            Err(problems) => Output::ok(json!({
                "target": target.as_str(),
                "ok": false,
                "problems": problems_json(&problems),
            }))
            .then(Next::new(
                "draft.lint",
                json!({"target": target.as_str()}),
                "after correcting the files the problems name",
            )),
        };
        Ok(output)
    }
);

impl Kernel {
    /// Stores a plan as a version of its target, makes it the current one,
    /// and writes the audit entry, in one transaction.
    pub(crate) async fn store_version(
        &self,
        encoded: EncodedPlan,
        target: TargetName,
        version: Version,
        by: PrincipalId,
        gate_evidence: Option<String>,
    ) -> Result<Digest, Failure> {
        let workspace = self.installation()?.workspace;
        let hash = encoded.hash;
        let detail = format!("version {version}, plan {hash}");
        let subject = target.to_string();
        self.store()?
            .write(Durability::Synced, move |tx| {
                tx.versions().put(&PublishedVersion {
                    workspace,
                    target: target.clone(),
                    version,
                    plan: encoded,
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

    /// Stores a plan built by hand as the current version of its target,
    /// so that a test of a crate above this one has a published tool to
    /// call without the compiler. Never part of the product: a plan reaches
    /// the store of a product only through `version.publish`.
    ///
    /// # Errors
    ///
    /// The plan's own check and encoding failures; the store's failures.
    #[cfg(any(test, feature = "testing"))]
    pub async fn publish_plan(&self, plan: &a2m_core::Plan) -> Result<Digest, Failure> {
        plan.check()?;
        let by = self.installation()?.user;
        self.store_version(
            plan.encode()?,
            plan.header.target.clone(),
            plan.header.version,
            by,
            None,
        )
        .await
    }
}

handler!(
    /// `version.publish`: compiles the drafts for publishing and stores the
    /// plan as an immutable version. The dispatcher has asked for proof
    /// that a person is present before this runs.
    version_publish,
    |kernel, call| {
        let target = TargetName::new(call.required("target")?)?;
        let workspace = call.caller.principal.workspace;
        let version = chosen_version(kernel, workspace, &target, call.text("version")).await?;
        let compiled = compiled_target(kernel, &call, &target, CompileMode::Publish, version)
            .await?
            .map_err(Failure::from)?;
        // A publish a person was shown on a page carries the hash of the
        // plan they read: drafts changed since are not published.
        if let Some(pinned) = call.text("plan_hash")
            && pinned != compiled.encoded.hash.to_string()
        {
            return Err(codes::stale(
                codes::DRAFTS_CHANGED,
                "The drafts changed after the publish was asked for, so they compile to another plan than the one shown. Nothing was published.",
                "Ask for the publish again; the person reads the new plan on a new page.",
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
            .plan
            .tools
            .iter()
            .map(|tool| tool.name.to_string())
            .collect();
        let hash = kernel
            .store_version(
                compiled.encoded,
                target.clone(),
                version,
                call.caller.principal.principal.id,
                stored_evidence,
            )
            .await?;
        let mut output = Output::ok(json!({
            "target": target.as_str(),
            "version": version.to_string(),
            "plan_hash": hash.to_string(),
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
    /// person surface as a person's act, with the plan it will store, and
    /// the person decides on its page (decision 0163).
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
        let project = (call.project.clone(), call.project_refused.clone());
        kernel.ask_person(&def, &call.caller, input, project).await
    }
);
