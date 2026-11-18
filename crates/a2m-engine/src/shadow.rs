//! A binding in shadow beside a live read (spec section 201, decision 0207).
//!
//! When the verified binding of a read has returned, each binding of the
//! operation whose status is `shadow` reads the same inputs, under the same
//! session, in a context of its own whose posture keeps the driver's own
//! read-only guard on and makes the handle refuse every step that commits.
//! What it returns is normalized output by output and compared exactly with
//! what the verified binding returned; the comparison, which names outputs
//! and never values, goes to the control plane, which keeps it in the
//! ledger by the shadow binding's digest.
//!
//! A shadow never changes the call. Its steps leave no record in the run's
//! trace; its failure is counted as "not compared"; it is cancelled when it
//! passes `EngineSettings::shadow_limit`, which is all it may hold the call
//! up by; and it runs at most once per operation in a job. The shadow of a
//! write is not built: a writing tool's job runs none.

use a2m_core::{FailureClass, OperationPlan, Values};
use a2m_ledger::{Difference, DifferingOutput, ShadowOutcome};
use a2m_provider::Posture;

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

/// Runs every binding in shadow of `found` beside the read the binding at
/// `verified` just made, which returned `outputs`, and hands each
/// comparison to the control plane. Whatever happens here, the caller is
/// given `outputs`.
pub(crate) async fn beside(
    run: &mut Run<'_>,
    loaded: &Loaded,
    found: &OperationPlan,
    verified: usize,
    scope: &Scope,
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
    let limit = run.engine.deps.settings.shadow_limit;
    for (position, candidate) in candidates {
        // A shadow is started only while the lease leaves it its whole
        // bound, and never after a cancel or a pause.
        if run.checkpoint(limit).is_err() {
            return;
        }
        let Some(shadow) = BindingPin::of(&loaded.plan, found, position) else {
            continue;
        };
        let outcome = match candidate {
            Err(failure) => not_compared(&failure),
            Ok(candidate) => {
                let aside = run.set_aside();
                let observed =
                    tokio::time::timeout(limit, read(run, found, &candidate, scope)).await;
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
        // A comparison that cannot be kept is lost; the call is not.
        let _ = run
            .control
            .shadow(ShadowCompared {
                operation: found.name.clone(),
                shadow,
                verified: verified.clone(),
                writes: false,
                outcome,
            })
            .await;
    }
}

/// The shadow binding's read, in a context of its own. `None` when a step
/// found nothing.
async fn read(
    run: &mut Run<'_>,
    found: &OperationPlan,
    candidate: &Candidate,
    scope: &Scope,
) -> Result<Option<Values>, a2m_core::Failure> {
    let binding = &found.bindings[candidate.binding];
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
