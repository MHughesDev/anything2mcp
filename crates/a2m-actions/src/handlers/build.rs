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

use super::handler;
use crate::codes;
use crate::dispatch::{Call, Next, Output};
use crate::kernel::{Kernel, note_published, published_targets};

/// The folder of a project that holds one folder per target.
const TARGETS_FOLDER: &str = "targets";

/// The project folder of a call, as a checked path. The folder a local
/// command was run in is the one root a project's files may be read from
/// and written to.
fn project(call: &Call) -> Result<CheckedPath, Failure> {
    let folder = call.project_folder()?;
    let name = RootName::new("project")?;
    let mut roots = Roots::none();
    roots.add(name.clone(), folder, Access::Write)?;
    roots.folder(&name, Access::Write)
}

/// The folder of one target of the project.
fn target_folder(call: &Call, target: &TargetName) -> Result<CheckedPath, Failure> {
    project(call)?.join(&format!("{TARGETS_FOLDER}/{target}"))
}

fn problems_json(problems: &Diagnostics) -> Vec<Json> {
    match problems.to_json() {
        Json::Array(items) => items,
        _ => Vec::new(),
    }
}

/// Loads and compiles the target's drafts.
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

handler!(
    /// `target.add`: makes the target's folder and a first file to fill in.
    target_add,
    |_kernel, call| {
        let target = TargetName::new(call.required("name")?)?;
        let folder = target_folder(&call, &target)?;
        let file = folder.join("target.yaml")?;
        if file.exists() {
            return Err(codes::input(
                codes::SCHEMA_INVALID,
                format!("The project already has a target named `{target}`."),
                "Choose another name, or work on the target that is there.",
            ));
        }
        folder.create_dir_all()?;
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
        file.write(text.as_bytes())?;
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
        let folder = target_folder(&call, &target)?;
        let output = match compile_target(kernel, &folder, CompileMode::Draft, DRAFT_VERSION) {
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
        let folder = target_folder(&call, &target)?;
        let workspace = call.caller.principal.workspace;
        let listed = target.clone();
        let earlier = kernel
            .store()?
            .read(move |tx| tx.versions().list(&workspace, &listed))
            .await?;
        let version = match call.text("version") {
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
        let compiled = compile_target(kernel, &folder, CompileMode::Publish, version)
            .map_err(Failure::from)?;
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
            )
            .await?;
        let mut output = Output::ok(json!({
            "target": target.as_str(),
            "version": version.to_string(),
            "plan_hash": hash.to_string(),
            "tools": tools,
            "current": true,
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
    /// `target.list`: the targets with a published version, and the
    /// versions of each.
    target_list,
    |kernel, call| {
        let workspace = call.caller.principal.workspace;
        let targets = kernel
            .store()?
            .read(move |tx| {
                let mut out = Vec::new();
                for target in published_targets(tx, &workspace)? {
                    let versions = tx.versions().list(&workspace, &target)?;
                    out.push((target, versions));
                }
                Ok(out)
            })
            .await?;
        let listed: Vec<Json> = targets
            .iter()
            .map(|(target, versions)| {
                json!({
                    "target": target.as_str(),
                    "versions": versions.iter().map(|row| json!({
                        "version": row.version.to_string(),
                        "plan_hash": row.plan_hash.to_string(),
                        "current": row.current,
                    })).collect::<Vec<_>>(),
                })
            })
            .collect();
        Ok(Output::ok(json!({"targets": listed})))
    }
);
