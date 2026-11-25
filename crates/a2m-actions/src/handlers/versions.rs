//! Which published version of a target runs now: `version.rollback`
//! (decision 0999).
//!
//! A rollback stores nothing new. It moves the mark of the version that runs
//! now to another version a person published, writes the audit entry in the
//! same transaction, and then reads the store back: the answer says the
//! version is current only when the store says so.

use a2m_core::{TargetName, Version};
use a2m_ledger::{AuditEntry, Durability};
use serde_json::json;

use super::handler;
use crate::codes;
use crate::dispatch::{Next, Output};

/// What the transaction of a rollback found and did.
struct Moved {
    hash: a2m_core::Digest,
    previous: Option<Version>,
    changed: bool,
}

handler!(
    /// `version.rollback`: makes another published version of a target the
    /// one that runs now. A person's act: the dispatcher has asked for
    /// proof that a person is present before this runs. Runs already under
    /// way keep the compiled target they started with; a watch tied to
    /// another version pauses or ends, as after a publish.
    version_rollback,
    |kernel, call| {
        let target = TargetName::new(call.required("target")?)?;
        let version = call
            .required("version")?
            .parse::<Version>()
            .map_err(|_| {
                codes::input(
                    codes::SCHEMA_INVALID,
                    "`version` is not three numbers joined by dots.",
                    "Write a version as, for example, `1.0.0`; `target.status` lists the published ones.",
                )
            })?;
        let workspace = kernel.installation()?.workspace;
        let by = call.caller.principal.principal.id;
        let (named, chosen) = (target.clone(), version);
        let moved = kernel
            .store()?
            .write(Durability::Synced, move |tx| {
                let rows = tx.versions().list(&workspace, &named)?;
                let Some(row) = rows.iter().find(|row| row.version == chosen) else {
                    return Err(codes::input(
                        codes::NOT_FOUND,
                        format!("The target `{named}` has no published version `{chosen}`."),
                        "Name a published version; `target.status` lists them.",
                    ));
                };
                let hash = row.compiled_target_hash;
                let previous = rows.iter().find(|row| row.current);
                if row.current {
                    // Already the one that runs: nothing is changed, and a
                    // call made again after a lost answer finds it so.
                    return Ok(Moved {
                        hash,
                        previous: None,
                        changed: false,
                    });
                }
                let previous = previous.map(|row| (row.version, row.compiled_target_hash));
                tx.versions().make_current(&workspace, &named, &hash)?;
                let detail = json!({
                    "version": chosen.to_string(),
                    "compiled_target_hash": hash.to_string(),
                    "previous": previous.map(|(version, _)| version.to_string()),
                    "previous_compiled_target_hash": previous.map(|(_, hash)| hash.to_string()),
                });
                tx.audit().append(&AuditEntry {
                    workspace,
                    actor: by.to_string(),
                    action: "version.rollback".to_owned(),
                    subject: named.to_string(),
                    detail: detail.to_string(),
                })?;
                Ok(Moved {
                    hash,
                    previous: previous.map(|(version, _)| version),
                    changed: true,
                })
            })
            .await?;
        // Proven after: the store is read again, and the version is called
        // current only when the store holds it as current.
        let named = target.clone();
        let now = kernel
            .store()?
            .read(move |tx| tx.versions().current(&workspace, &named))
            .await?;
        if now != Some(moved.hash) {
            return Err(codes::stale(
                codes::VERSION_MOVED,
                format!(
                    "Version `{version}` of `{target}` was made current, and the store now holds another version as current."
                ),
                "Call `target.status` to see which version is current, and roll back again if it is not the one wanted.",
            ));
        }
        if moved.changed {
            // A watch is tied to the version its person confirmed: every
            // watch of another version pauses or ends now, as after a
            // publish (decision 0262).
            kernel.watches_after_publish(&target, moved.hash).await;
        }
        let mut result = json!({
            "target": target.as_str(),
            "version": version.to_string(),
            "compiled_target_hash": moved.hash.to_string(),
            "changed": moved.changed,
            "current": true,
        });
        if let (Some(previous), Some(map)) = (moved.previous, result.as_object_mut()) {
            map.insert("previous".to_owned(), json!(previous.to_string()));
        }
        let mut output = Output::ok(result);
        if !moved.changed {
            output = output.warn(format!(
                "Version `{version}` of `{target}` was already the one that runs now; nothing was changed."
            ));
        }
        Ok(output.then(Next::new(
            "tool.search",
            json!({"target": target.as_str()}),
            "to see the tools of the version that runs now",
        )))
    }
);
