//! `assertion.confirm` (decision 0999): a person confirms, on the engine's
//! page, the reads a target's drafts assert, so that a try of the drafts in
//! an environment of kind `test` may let a page send them.
//!
//! What is confirmed is a digest of exactly what the page showed of each
//! asserted read: the target, the file kind, the entry, the surface, the
//! method, the path, every field of the body with its kind and the values
//! its `enum` allows, and the address the surface leads to in each
//! environment. The asking pins the digests; the press compiles the drafts
//! again and stores nothing when they changed. The confirmations are kept
//! in the engine's store, by target, and no target file holds one.
//!
//! When the kernel keeps the drafts for a trial, it hands the engine the
//! entries a confirmation covers ([`Kernel::confirmed_reads_of`]); the
//! engine hands them to the driver in `PrepareContext::confirmed`.

use std::collections::BTreeSet;

use a2m_core::{CompiledTarget, Digest, Failure, FieldHasher, TargetName};
use a2m_files::{AssertedReview, CompileMode, Compiled, DRAFT_VERSION};
use a2m_ledger::Durability;
use a2m_provider::ConfirmedReads;
use serde_json::{Map, Value as Json, json};

use super::build::compile_in;
use super::handler;
use crate::codes;
use crate::kernel::{Kernel, fact, set_fact};

/// The most confirmations kept for one target; the oldest go first.
pub(crate) const CONFIRMATIONS_KEPT: usize = 64;

/// The name of the store's fact that holds a target's confirmations.
fn fact_name(target: &TargetName) -> String {
    format!("confirmed_assertions:{target}")
}

/// Where the surface of `asserted` leads, in the compiled target's own
/// address and in each environment it declares, each `(environment,
/// address)`; the first has no environment.
fn addresses(compiled_target: &CompiledTarget, asserted: &AssertedReview) -> Vec<(String, String)> {
    let Some(index) = compiled_target
        .surfaces
        .iter()
        .position(|surface| surface.name == asserted.surface)
    else {
        return Vec::new();
    };
    let own = compiled_target.surfaces[index].address.clone();
    let mut found = vec![(String::new(), own.clone())];
    for environment in &compiled_target.environments {
        let address = environment
            .addresses
            .iter()
            .find(|given| given.surface.get() == index)
            .map_or_else(|| own.clone(), |given| given.address.clone());
        found.push((environment.name.as_str().to_owned(), address));
    }
    found
}

/// The digest of one asserted read as a page shows it (decision 0999,
/// point 2).
pub(crate) fn digest_of(
    target: &TargetName,
    compiled_target: &CompiledTarget,
    asserted: &AssertedReview,
) -> Digest {
    let mut hasher = FieldHasher::new();
    hasher.field("purpose", b"anything2mcp asserted read confirmed 1");
    hasher.field("target", target.as_str().as_bytes());
    hasher.field("kind", asserted.kind.as_str().as_bytes());
    hasher.field("surface", asserted.surface.as_str().as_bytes());
    hasher.field("entry", asserted.read.entry.as_bytes());
    hasher.field("method", asserted.read.method.as_bytes());
    hasher.field("path", asserted.read.path.as_bytes());
    for field in &asserted.read.fields {
        hasher.field("field", field.name.as_bytes());
        hasher.field("field kind", field.kind.as_bytes());
        for value in &field.one_of {
            hasher.field("field enum", value.as_bytes());
        }
        hasher.field("field end", b"");
    }
    for (environment, address) in addresses(compiled_target, asserted) {
        hasher.field("environment", environment.as_bytes());
        hasher.field("address", address.as_bytes());
    }
    hasher.finish()
}

/// Every asserted read of the compiled drafts with its digest.
fn asserted_of<'c>(
    target: &TargetName,
    compiled: &'c Compiled,
) -> Vec<(&'c AssertedReview, Digest)> {
    compiled
        .review
        .asserted
        .iter()
        .map(|asserted| {
            (
                asserted,
                digest_of(target, &compiled.compiled_target, asserted),
            )
        })
        .collect()
}

/// The lines a person reads of one asserted read, in the words the publish
/// page uses (decision 0394), with every address it leads to.
fn lines_of(compiled_target: &CompiledTarget, asserted: &AssertedReview) -> String {
    let fields = if asserted.read.fields.is_empty() {
        "no body".to_owned()
    } else {
        asserted
            .read
            .fields
            .iter()
            .map(|field| {
                if field.one_of.is_empty() {
                    format!("`{}`, a `{}`, any value", field.name, field.kind)
                } else {
                    format!(
                        "`{}`, a `{}`, one of {}",
                        field.name,
                        field.kind,
                        field
                            .one_of
                            .iter()
                            .map(|value| format!("`{value}`"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }
            })
            .collect::<Vec<_>>()
            .join("; ")
    };
    let places = addresses(compiled_target, asserted)
        .into_iter()
        .skip(1)
        .map(|(environment, address)| format!("`{address}` in `{environment}`"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "`{}` `{}` on the surface `{}` ({places}), asserted by the `{}` `{}`. Its body may carry: {fields}. Note: {}.",
        asserted.read.method,
        asserted.read.path,
        asserted.surface,
        asserted.kind,
        asserted.read.entry,
        a2m_files::ASSERTED_NOTICE,
    )
}

/// What the press does, said once below the reads.
const PRESS_DOES: &str = "Confirming lets a try of these drafts, in an environment of kind `test` and in the engine's sandbox, let a page send each request above, with a body that fits its closed schema; every other request a page makes is still refused. A change to a request's method, path, body, or address undoes its confirmation. Nothing is published: a publish still asks you on its own page.";

fn nothing_asserted(target: &TargetName) -> Failure {
    codes::input(
        codes::NOTHING_ASSERTED,
        format!("The drafts of `{target}` assert no read, so there is nothing to confirm."),
        "A person asserts a read by writing `effect: read` and `read_asserted: true` in its entry, with an `enum` on the field that tells a read from a write; then ask again.",
    )
}

fn texts(value: Option<&Json>) -> BTreeSet<String> {
    value
        .and_then(Json::as_array)
        .map(|list| {
            list.iter()
                .filter_map(Json::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

impl Kernel {
    /// The confirmations stored for `target`, as digests' texts, oldest
    /// first.
    pub(crate) async fn confirmations(&self, target: &TargetName) -> Result<Vec<String>, Failure> {
        let name = fact_name(target);
        let text = self.store()?.read(move |tx| fact(tx, &name)).await?;
        Ok(text
            .and_then(|text| serde_json::from_str::<Vec<String>>(&text).ok())
            .unwrap_or_default())
    }

    /// Adds `digests` to the confirmations of `target`, keeping the newest
    /// [`CONFIRMATIONS_KEPT`].
    async fn confirm(&self, target: &TargetName, digests: Vec<String>) -> Result<(), Failure> {
        let name = fact_name(target);
        self.store()?
            .write(Durability::Synced, move |tx| {
                let mut held = fact(tx, &name)?
                    .and_then(|text| serde_json::from_str::<Vec<String>>(&text).ok())
                    .unwrap_or_default();
                held.retain(|kept| !digests.contains(kept));
                held.extend(digests);
                let over = held.len().saturating_sub(CONFIRMATIONS_KEPT);
                held.drain(..over);
                let text = serde_json::to_string(&held).map_err(|_| {
                    codes::busy(
                        a2m_ledger::codes::STORE_UNAVAILABLE,
                        "The confirmations could not be written down.",
                        "Report this; it is a fault in the engine.",
                    )
                })?;
                set_fact(tx, &name, &text)
            })
            .await
    }

    /// The asserted reads of the compiled drafts that a stored confirmation
    /// covers, for the engine to hand a trial's driver (decision 0999,
    /// point 3). A store that cannot be read covers none.
    pub(crate) async fn confirmed_reads_of(
        &self,
        target: &TargetName,
        compiled_target: &CompiledTarget,
        asserted: &[AssertedReview],
    ) -> ConfirmedReads {
        if asserted.is_empty() {
            return ConfirmedReads::none();
        }
        let held: BTreeSet<String> = self
            .confirmations(target)
            .await
            .unwrap_or_default()
            .into_iter()
            .collect();
        ConfirmedReads::of(
            asserted
                .iter()
                .filter(|read| held.contains(&digest_of(target, compiled_target, read).to_string()))
                .map(|read| {
                    (
                        read.kind.as_str().to_owned(),
                        read.surface.as_str().to_owned(),
                        read.read.entry.clone(),
                    )
                })
                .collect::<Vec<_>>(),
        )
    }

    /// The input of a confirmation with the digests it rests on pinned, and
    /// the lines its page shows: each asserted read of the drafts as they
    /// are now, and what the press does.
    pub(crate) async fn assertion_lines(
        &self,
        mut input: Map<String, Json>,
        project: &(Option<std::path::PathBuf>, Option<Failure>),
    ) -> Result<(Map<String, Json>, Vec<String>), Failure> {
        let folder = match project {
            (Some(folder), _) => folder.clone(),
            (None, Some(refused)) => return Err(refused.clone()),
            (None, None) => {
                return Err(codes::input(
                    codes::PATH_NOT_ALLOWED,
                    "A confirmation works on a project folder, and the call names none.",
                    "Run it inside a project folder, or pass `--project` with the folder's full path.",
                ));
            }
        };
        let target = TargetName::new(input.get("target").and_then(Json::as_str).unwrap_or(""))?;
        let compiled = compile_in(self, folder, &target, CompileMode::Draft, DRAFT_VERSION)
            .await?
            .map_err(Failure::from)?;
        let asserted = asserted_of(&target, &compiled);
        if asserted.is_empty() {
            return Err(nothing_asserted(&target));
        }
        let mut lines: Vec<String> = asserted
            .iter()
            .map(|(read, _)| lines_of(&compiled.compiled_target, read))
            .collect();
        lines.push(PRESS_DOES.to_owned());
        let digests: BTreeSet<String> = asserted
            .iter()
            .map(|(_, digest)| digest.to_string())
            .collect();
        input.insert("assertions".to_owned(), json!(digests));
        Ok((input, lines))
    }
}

handler!(
    /// `assertion.confirm`: the person confirms the reads the drafts of a
    /// target assert (decision 0999). Only with a proof of presence; the
    /// drafts are compiled again, and the digests must be the ones the
    /// page showed.
    assertion_confirm,
    |kernel, call| {
        if call.presence.is_none() {
            return Err(codes::policy(
                codes::PERSON_REQUIRED,
                "`assertion.confirm` is a person's act, and no proof of presence was given.",
                "Confirm the reads on the engine's page, past the prompt, or at a terminal yourself.",
            ));
        }
        let target = TargetName::new(call.required("target")?)?;
        let folder = call.project_folder()?.to_path_buf();
        let compiled = compile_in(kernel, folder, &target, CompileMode::Draft, DRAFT_VERSION)
            .await?
            .map_err(Failure::from)?;
        let asserted = asserted_of(&target, &compiled);
        if asserted.is_empty() {
            return Err(nothing_asserted(&target));
        }
        let now: BTreeSet<String> = asserted
            .iter()
            .map(|(_, digest)| digest.to_string())
            .collect();
        let pinned = texts(call.input.get("assertions"));
        if call.input.contains_key("assertions") && pinned != now {
            return Err(codes::stale(
                codes::ASSERTION_CHANGED,
                format!(
                    "The reads the drafts of `{target}` assert changed after the page was shown: what would be confirmed is not what was read. Nothing was confirmed."
                ),
                "Ask for the confirmation again, and confirm the reads as they are now.",
            ));
        }
        kernel
            .confirm(&target, now.iter().cloned().collect())
            .await?;
        let confirmed: Vec<Json> = asserted
            .iter()
            .map(|(read, digest)| {
                json!({
                    "kind": read.kind.as_str(),
                    "surface": read.surface.as_str(),
                    "entry": read.read.entry,
                    "method": read.read.method,
                    "path": read.read.path,
                    "digest": digest.to_string(),
                })
            })
            .collect();
        Ok(crate::dispatch::Output::ok(json!({
            "target": target.as_str(),
            "state": "confirmed",
            "confirmed": confirmed,
        })))
    }
);
