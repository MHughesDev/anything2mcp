//! Published tools: found by name, with the compiled target that holds them.

use std::sync::Arc;

use a2m_core::{
    CompiledTarget, CompiledTool, Digest, EffectClass, Failure, Hazard, OperationName, TargetName,
    ToolRuns,
};

use crate::codes;
use crate::kernel::{Kernel, published_targets};

/// A published tool, with the compiled target it is in.
#[derive(Debug, Clone)]
pub(crate) struct FoundTool {
    pub(crate) target: TargetName,
    pub(crate) compiled_target_hash: Digest,
    pub(crate) compiled_target: Arc<CompiledTarget>,
    pub(crate) tool: CompiledTool,
}

impl FoundTool {
    /// The effect class and hazard of what the tool runs.
    pub(crate) fn effect(&self) -> (EffectClass, Hazard) {
        match self.tool.runs {
            ToolRuns::Operation(index) => self
                .compiled_target
                .operation(index)
                .map(|operation| (operation.effect, operation.hazard)),
            ToolRuns::Pipeline(index) => self
                .compiled_target
                .pipeline(index)
                .map(|pipeline| (pipeline.effect, pipeline.hazard)),
        }
        // A tool whose compiled target does not hold what it runs is treated
        // as the most dangerous kind; the engine refuses to run it.
        .unwrap_or((EffectClass::Irreversible, Hazard::None))
    }

    /// `operation` or `pipeline`.
    pub(crate) const fn kind(&self) -> &'static str {
        match self.tool.runs {
            ToolRuns::Operation(_) => "operation",
            ToolRuns::Pipeline(_) => "pipeline",
        }
    }
}

impl Kernel {
    /// The compiled target with this hash, read from the store once and kept.
    /// The store hashes the bytes again, and decoding checks the hash once
    /// more, so a compiled target that fails its check is unavailable alone.
    ///
    /// # Errors
    ///
    /// `input`/`not_found`; `busy`/`store_corrupt`; the compiled target's own
    /// decode failures.
    pub(crate) async fn compiled_target_by_hash(
        &self,
        target: &TargetName,
        hash: &Digest,
    ) -> Result<Arc<CompiledTarget>, Failure> {
        // A compiled target is kept under its target as well as its hash, and
        // only after the store gave it for this installation's workspace, so
        // a hash alone never finds a compiled target of another target.
        let key = (target.clone(), *hash);
        if let Some(compiled_target) = self
            .inner
            .compiled_targets
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&key)
        {
            return Ok(Arc::clone(compiled_target));
        }
        // A drafted compiled target a trial runs is kept in memory only, and
        // found here by the runner and its control; it is never a version.
        if let Some(compiled_target) = self
            .inner
            .drafts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .find(|(kept, _)| *kept == key)
            .map(|(_, compiled_target)| Arc::clone(compiled_target))
        {
            return Ok(compiled_target);
        }
        let workspace = self.installation()?.workspace;
        let (target, wanted) = (target.clone(), *hash);
        let encoded = self
            .store()?
            .read(move |tx| tx.versions().load(&workspace, &target, &wanted))
            .await?;
        let compiled_target = Arc::new(CompiledTarget::decode(&encoded.bytes, hash)?);
        let mut compiled_targets = self
            .inner
            .compiled_targets
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Bounded: when it is full it is emptied, and a compiled target is
        // read from the store once more.
        if compiled_targets.len() >= crate::kernel::COMPILED_TARGETS_KEPT {
            compiled_targets.clear();
        }
        compiled_targets.insert(key, Arc::clone(&compiled_target));
        drop(compiled_targets);
        Ok(compiled_target)
    }

    /// Keeps a drafted compiled target in memory for a trial's run, the
    /// newest [`DRAFTS_KEPT`](crate::kernel::DRAFTS_KEPT) of them.
    pub(crate) fn keep_draft(
        &self,
        target: &TargetName,
        hash: Digest,
        compiled_target: Arc<CompiledTarget>,
    ) {
        let mut drafts = self
            .inner
            .drafts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let key = (target.clone(), hash);
        drafts.retain(|(kept, _)| *kept != key);
        if drafts.len() >= crate::kernel::DRAFTS_KEPT {
            drafts.remove(0);
        }
        drafts.push((key, compiled_target));
    }

    /// The current version of every published target, in order of target.
    /// A target whose compiled target cannot be read is left out, and named.
    ///
    /// # Errors
    ///
    /// The store's failures.
    pub(crate) async fn current_compiled_targets(
        &self,
    ) -> Result<
        (
            Vec<(TargetName, Digest, Arc<CompiledTarget>)>,
            Vec<TargetName>,
        ),
        Failure,
    > {
        let workspace = self.installation()?.workspace;
        let current = self
            .store()?
            .read(move |tx| {
                let mut found = Vec::new();
                for target in published_targets(tx, &workspace)? {
                    if let Some(hash) = tx.versions().current(&workspace, &target)? {
                        found.push((target, hash));
                    }
                }
                Ok(found)
            })
            .await?;
        let mut compiled_targets = Vec::new();
        let mut unreadable = Vec::new();
        for (target, hash) in current {
            match self.compiled_target_by_hash(&target, &hash).await {
                Ok(compiled_target) => compiled_targets.push((target, hash, compiled_target)),
                Err(_) => unreadable.push(target),
            }
        }
        Ok((compiled_targets, unreadable))
    }

    /// The published tool of that name, in the current version of the
    /// target it belongs to.
    ///
    /// # Errors
    ///
    /// `input`/`not_found` when no current version holds such a tool.
    pub(crate) async fn find_tool(&self, name: &OperationName) -> Result<FoundTool, Failure> {
        let (compiled_targets, _) = self.current_compiled_targets().await?;
        for (target, compiled_target_hash, compiled_target) in compiled_targets {
            if !name.belongs_to(&target) {
                continue;
            }
            if let Some(tool) = compiled_target.tool(name) {
                return Ok(FoundTool {
                    tool: tool.clone(),
                    target,
                    compiled_target_hash,
                    compiled_target,
                });
            }
        }
        Err(codes::input(
            codes::NOT_FOUND,
            format!("No published tool is named `{name}`."),
            "Call `tool.search` to list the published tools.",
        ))
    }
}
