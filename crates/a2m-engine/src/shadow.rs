//! A binding in shadow beside a live read (spec section 201, decisions 0207
//! and 0251).
//!
//! When the verified binding of a read has returned, each binding of the
//! operation whose status is `shadow` reads the same inputs, under the same
//! session, in a context of its own whose posture keeps the driver's own
//! read-only guard on and makes the handle refuse every step that commits.
//! What it returns is normalized output by output and compared exactly with
//! what the verified binding returned; the comparison, which names outputs
//! and never values, goes to the control plane, which keeps it in the
//! ledger by the shadow binding's digest, with the keyed digest of the
//! call's inputs, the environment, and the verified binding it was compared
//! against.
//!
//! A shadow never changes the call. Its steps leave no record in the run's
//! trace; its failure is counted as "not compared"; and it runs at most once
//! per operation in a job. Every shadow of a job shares one budget of time,
//! `EngineSettings::shadow_limit`, which the call's own time limit and lease
//! also bound: their reads take at most three quarters of it, and the
//! keeping of their comparisons, which happens only after the call's result
//! was sent, the rest. A shadow never asks for a session the live call does
//! not hold, and never takes a caller's share of the surface's rate budget.
//! The shadow of a write is not built: a writing tool's job runs none.

use std::sync::atomic::Ordering;
use std::time::Duration;

use a2m_core::{
    BindingStatus, Environment, EnvironmentKind, FailureClass, OperationPlan, Plan, Values,
};
use a2m_ledger::{Difference, DifferingOutput, ShadowOutcome};
use a2m_provider::Posture;
use tokio::time::Instant;

use crate::binding::BindingPin;
use crate::codes;
use crate::control::ShadowCompared;
use crate::loaded::{Loaded, Scope};
use crate::run::{Candidate, Run};
use crate::tool::{observe, output_value};

/// The posture a binding in shadow is prepared in: the driver's read-only
/// guard is on, and the handle refuses every step that commits.
const SHADOW_POSTURE: Posture = Posture::Sandbox {
    test_records: false,
};

/// The share of the shadow budget, in quarters, the reads may take; the
/// rest is kept for keeping their comparisons.
const READ_QUARTERS: u32 = 3;

/// The environments the evidence for promoting a binding of `plan` must
/// come from (decision 0251): every environment of kind `production`, or,
/// in a plan that has none, every environment. Matches in a test
/// environment, whose data a caller may have written, never make a binding
/// promotable for production.
#[must_use]
pub fn shadow_evidence_environments(plan: &Plan) -> Vec<Environment> {
    let production: Vec<Environment> = plan
        .environments
        .iter()
        .filter(|environment| environment.kind == EnvironmentKind::Production)
        .map(|environment| environment.name.clone())
        .collect();
    if production.is_empty() {
        plan.environments
            .iter()
            .map(|environment| environment.name.clone())
            .collect()
    } else {
        production
    }
}

/// The verified binding a binding in shadow of `operation` is compared
/// against on a live call: the first `verified` one, which acts. `None`
/// when none is verified. Only matches against it count toward promoting
/// a binding beside it (decision 0251).
#[must_use]
pub fn verified_binding(plan: &Plan, operation: &OperationPlan) -> Option<BindingPin> {
    let position = operation
        .bindings
        .iter()
        .position(|binding| binding.status == BindingStatus::Verified)?;
    BindingPin::of(plan, operation, position)
}

/// The end of the job's shadow budget: `shadow_limit` from now, or sooner
/// when the call's own time limit or lease ends first.
fn deadline(run: &Run<'_>) -> Instant {
    let mut left = run.engine.deps.settings.shadow_limit;
    if let Some(limit) = run.time_limit {
        left = left.min(limit.saturating_sub(run.engine.deps.clock.elapsed()));
    }
    if let Some(lease) = run.job.lease.remaining() {
        left = left.min(lease);
    }
    Instant::now() + left
}

/// The moment the reads of the budget ending at `end`, which began at
/// `start`, must end.
fn reads_end(start: Instant, end: Instant) -> Instant {
    let whole = end.saturating_duration_since(start);
    start + whole / 4 * READ_QUARTERS
}

/// Runs every binding in shadow of `found` beside the read the binding at
/// `verified` just made with `inputs`, which returned `outputs`, and holds
/// each comparison in the run until [`keep`]. Whatever happens here, the
/// caller is given `outputs`.
pub(crate) async fn beside(
    run: &mut Run<'_>,
    loaded: &Loaded,
    found: &OperationPlan,
    verified: usize,
    scope: &Scope,
    inputs: &Values,
    outputs: &Values,
) {
    if !run.shadow_allowed || run.shadowed.contains(&found.name) || !run.control.keeps_shadow() {
        return;
    }
    let Ok(candidates) = run.shadow_candidates(found, scope) else {
        return;
    };
    if candidates.is_empty() {
        return;
    }
    run.shadowed.push(found.name.clone());
    let Some(verified) = BindingPin::of(&loaded.plan, found, verified) else {
        return;
    };
    // One budget for every shadow of the job, set when the first starts:
    // the reads end where it leaves room to keep their comparisons.
    let (reads_until, _) = match run.shadow_budget {
        Some(budget) => budget,
        None => {
            let (start, end) = (Instant::now(), deadline(run));
            let budget = (reads_end(start, end), end);
            run.shadow_budget = Some(budget);
            budget
        }
    };
    for (position, candidate) in candidates {
        // A shadow is started only inside the budget, and never after a
        // cancel or a pause.
        if Instant::now() >= reads_until || run.checkpoint(Duration::ZERO).is_err() {
            return;
        }
        let Some(shadow) = BindingPin::of(&loaded.plan, found, position) else {
            continue;
        };
        let before = run.granted.load(Ordering::Relaxed);
        let outcome = match candidate {
            Err(failure) => not_compared(&failure),
            Ok(candidate) => {
                let aside = run.set_aside();
                run.shadow_reading = true;
                let observed =
                    tokio::time::timeout_at(reads_until, read(run, found, &candidate, scope)).await;
                run.shadow_reading = false;
                run.restore(aside);
                match observed {
                    Ok(Ok(raw)) => compare(loaded, found, outputs, raw.as_ref()),
                    Ok(Err(failure)) => not_compared(&failure),
                    Err(_) => {
                        // Its future was dropped wherever it was: what it
                        // held is not used again.
                        run.poison_posture(SHADOW_POSTURE);
                        ShadowOutcome::NotCompared {
                            class: FailureClass::Busy,
                            code: codes::SHADOW_TIME_EXCEEDED,
                        }
                    }
                }
            }
        };
        let spent = run.granted.load(Ordering::Relaxed).saturating_sub(before);
        run.shadow_compared.push(ShadowCompared {
            operation: found.name.clone(),
            shadow,
            verified: verified.clone(),
            writes: false,
            outcome,
            input: inputs.clone(),
            requests: u32::try_from(spent).unwrap_or(u32::MAX),
        });
    }
}

/// Keeps the job's comparisons, once the call's result was sent: within
/// what is left of the job's shadow budget, mismatches first, since
/// evidence against a binding is the one never to lose. A comparison that
/// is not kept within it, or whose keeping fails, is dropped and counted
/// in [`crate::Engine::shadow_comparisons_lost`]; the call is not changed.
pub(crate) async fn keep(run: &mut Run<'_>) {
    let mut compared = std::mem::take(&mut run.shadow_compared);
    if compared.is_empty() {
        return;
    }
    compared.sort_by_key(|one| match one.outcome {
        ShadowOutcome::Mismatched(_) => 0,
        ShadowOutcome::Matched => 1,
        ShadowOutcome::NotCompared { .. } => 2,
    });
    let end = run.shadow_budget.map_or_else(Instant::now, |(_, end)| end);
    for one in compared {
        let kept = tokio::time::timeout_at(end, run.control.shadow(one)).await;
        if !matches!(kept, Ok(Ok(()))) {
            run.engine.shadows_lost.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// The shadow binding's read, in a context of its own. `None` when a step
/// found nothing. It reads only through a session the live call already
/// holds: a shadow never asks the keeper for one, so it can never prompt a
/// person to sign in.
async fn read(
    run: &mut Run<'_>,
    found: &OperationPlan,
    candidate: &Candidate,
    scope: &Scope,
) -> Result<Option<Values>, a2m_core::Failure> {
    let binding = &found.bindings[candidate.binding];
    if run.loaded.plan.auth_of(binding.surface).is_some() && !run.holds_session_on(binding.surface)
    {
        return Err(codes::fail(
            FailureClass::Busy,
            codes::NO_BINDING,
            format!(
                "The binding in shadow through the mode `{}` reads through a surface whose session the call does not hold.",
                binding.mode
            ),
            "Nothing to do; a binding in shadow reads only through the session of the call it is compared beside.",
        ));
    }
    let held = run
        .hold_at(
            &found.name,
            &binding.mode,
            binding.surface,
            candidate,
            false,
            SHADOW_POSTURE,
        )
        .await?;
    let observed = observe(run, held, binding, scope, &[]).await;
    if observed.is_err() {
        run.poison(held);
    }
    observed
}

fn not_compared(failure: &a2m_core::Failure) -> ShadowOutcome {
    ShadowOutcome::NotCompared {
        class: failure.class(),
        code: failure.code().clone(),
    }
}

/// Compares what the shadow binding observed with the verified binding's
/// outputs, output by output in the operation's declared order, each
/// normalized by its field type: equal values, or the names of those that
/// differ and how. An output the shadow binding did not give is a
/// mismatch, and so is a read that found nothing.
fn compare(
    loaded: &Loaded,
    found: &OperationPlan,
    verified: &Values,
    observed: Option<&Values>,
) -> ShadowOutcome {
    let mut differing = Vec::new();
    for output in &found.outputs {
        let expected = verified.get(&output.name);
        let how = match observed.and_then(|observed| observed.get(&output.name)) {
            None if expected.is_none() => None,
            None => Some(Difference::Missing),
            Some(raw) => match output_value(loaded, found, output, raw) {
                Err(_) => Some(Difference::Unreadable),
                Ok(value) => (Some(&value) != expected).then_some(Difference::Differs),
            },
        };
        if let Some(how) = how {
            differing.push(DifferingOutput {
                name: output.name.clone(),
                how,
            });
        }
    }
    if differing.is_empty() {
        ShadowOutcome::Matched
    } else {
        ShadowOutcome::Mismatched(differing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reads_take_three_quarters_of_the_budget() {
        let start = Instant::now();
        let end = start + Duration::from_millis(2000);
        assert_eq!(reads_end(start, end), start + Duration::from_millis(1500));
        assert_eq!(reads_end(end, end), end);
    }
}
