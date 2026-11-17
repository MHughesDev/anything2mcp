//! Published tools: found by name, with the plan that holds them.

use std::sync::Arc;

use a2m_core::{
    Digest, EffectClass, Failure, Hazard, OperationName, Plan, TargetName, ToolPlan, ToolRuns,
};

use crate::codes;
use crate::kernel::{Kernel, published_targets};

/// A published tool, with the plan it is in.
#[derive(Debug, Clone)]
pub(crate) struct FoundTool {
    pub(crate) target: TargetName,
    pub(crate) plan_hash: Digest,
    pub(crate) plan: Arc<Plan>,
    pub(crate) tool: ToolPlan,
}

impl FoundTool {
    /// The effect class and hazard of what the tool runs.
    pub(crate) fn effect(&self) -> (EffectClass, Hazard) {
        match self.tool.runs {
            ToolRuns::Operation(index) => self
                .plan
                .operation(index)
                .map(|operation| (operation.effect, operation.hazard)),
            ToolRuns::Pipeline(index) => self
                .plan
                .pipeline(index)
                .map(|pipeline| (pipeline.effect, pipeline.hazard)),
        }
        // A tool whose plan does not hold what it runs is treated as the
        // most dangerous kind; the engine refuses to run it.
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
    /// The plan with this hash, read from the store once and kept. The
    /// store hashes the bytes again, and decoding checks the hash once
    /// more, so a plan that fails its check is unavailable alone.
    ///
    /// # Errors
    ///
    /// `input`/`not_found`; `busy`/`store_corrupt`; the plan's own decode
    /// failures.
    pub(crate) async fn plan_by_hash(
        &self,
        target: &TargetName,
        hash: &Digest,
    ) -> Result<Arc<Plan>, Failure> {
        // A plan is kept under its target as well as its hash, and only
        // after the store gave it for this installation's workspace, so a
        // hash alone never finds a plan of another target.
        let key = (target.clone(), *hash);
        if let Some(plan) = self
            .inner
            .plans
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&key)
        {
            return Ok(Arc::clone(plan));
        }
        let workspace = self.installation()?.workspace;
        let (target, wanted) = (target.clone(), *hash);
        let encoded = self
            .store()?
            .read(move |tx| tx.versions().load(&workspace, &target, &wanted))
            .await?;
        let plan = Arc::new(Plan::decode(&encoded.bytes, hash)?);
        let mut plans = self
            .inner
            .plans
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Bounded: when it is full it is emptied, and a plan is read from
        // the store once more.
        if plans.len() >= crate::kernel::PLANS_KEPT {
            plans.clear();
        }
        plans.insert(key, Arc::clone(&plan));
        drop(plans);
        Ok(plan)
    }

    /// The current version of every published target, in order of target.
    /// A target whose plan cannot be read is left out, and named.
    ///
    /// # Errors
    ///
    /// The store's failures.
    pub(crate) async fn current_plans(
        &self,
    ) -> Result<(Vec<(TargetName, Digest, Arc<Plan>)>, Vec<TargetName>), Failure> {
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
        let mut plans = Vec::new();
        let mut unreadable = Vec::new();
        for (target, hash) in current {
            match self.plan_by_hash(&target, &hash).await {
                Ok(plan) => plans.push((target, hash, plan)),
                Err(_) => unreadable.push(target),
            }
        }
        Ok((plans, unreadable))
    }

    /// The published tool of that name, in the current version of the
    /// target it belongs to.
    ///
    /// # Errors
    ///
    /// `input`/`not_found` when no current version holds such a tool.
    pub(crate) async fn find_tool(&self, name: &OperationName) -> Result<FoundTool, Failure> {
        let (plans, _) = self.current_plans().await?;
        for (target, plan_hash, plan) in plans {
            if !name.belongs_to(&target) {
                continue;
            }
            if let Some(tool) = plan.tool(name) {
                return Ok(FoundTool {
                    tool: tool.clone(),
                    target,
                    plan_hash,
                    plan,
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
