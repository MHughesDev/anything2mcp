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

/// The next step for drafts that compile, chosen on the gates a publish
/// runs (decision 0219): the same publish compile, which refuses an
/// operation with no offline test, and the same offline tests. Only drafts
/// that pass both are pointed at `ready`, the publish; otherwise the hint
/// names the step that can succeed. Nothing is stored: an offline test runs
/// in a store of its own.
pub(crate) async fn publish_next(kernel: &Kernel, call: &Call, target: &TargetName, ready: Next) -> Next {
    let tests = |when: String| Next::new("test.run", json!({"target": target.as_str()}), when);
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
        Ok(gates) if gates.passed => ready,
        Ok(_) => tests("to see which offline tests did not pass, and correct them before a publish".to_owned()),
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
    format!(
        "# The target `{target}`, with the one surface it was added with. Run `anything2mcp scout run {target}` next.\n\
         schema_version: 1\ntarget: {target}\ntitle: {target}\n\
         surfaces:\n  {kind}:\n    kind: {kind}\n    {key}: {address}\n\
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
        "Correct the address or the kind, and call `target.add` again.",
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
        )?;
        let text = first_target_file(&target, surface.as_ref());
        let declared = surface.is_some();
        let (kernel, named) = (kernel.clone(), target.clone());
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
                let set = kernel.providers().compiler_set();
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
                    "plan_hash": compiled.encoded.hash.to_string(),
                    "tools": compiled.plan.tools.iter().map(|tool| tool.name.to_string()).collect::<Vec<_>>(),
                }))
                .then(publish_next(kernel, &call, &target, ready).await)
            }
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
            .map_err(|problems| crate::offline::publish_refusal(&target, problems))?;
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
