//! The two budgets measured from the trace's own time marks (roadmap package
//! 1.9b, decision 0481): the policy check, the scheduling, and the ledger
//! write of a call, and getting a tab on a warm, signed-in browser.
//!
//! The harness's clock cannot tell these apart from the rest of a call, so
//! each iteration calls the tool through the product's commands, exports the
//! run's stored trace with `trace export`, and takes the number from it: the
//! end line's time marks for the first, and the `prepare_us` of the pass that
//! read through the `ui` binding for the second. The export is made after the
//! call and is in no number.

use std::time::Duration;

use a2m_logs::{PassLine, TimeMarks, TraceLine};
use serde_json::{Value, json};

use crate::error::BenchError;
use crate::harness::{Clock, Plan, ScenarioResult};
use crate::measure::{Reading, reading};
use crate::promotion::{MEASURED, TOOL};
use crate::stats::Stats;

/// The policy check, the scheduling, and the ledger write of one call, as the
/// engine marked them in the run's trace.
pub const CALL_OVERHEAD: &str = "call_overhead";
/// The time of a call outside the engine's run of it: the command front
/// door, the scheduler's claim and finish, and the trace's storing. Not
/// budgeted.
pub const CALL_OUTSIDE_RUN: &str = "call_outside_run";
/// The preparing of the `ui` binding's driver on a warm, signed-in browser:
/// the tab the read looks through, as the engine marked it in the trace.
pub const WARM_TAB: &str = "warm_tab";

/// What the call scenario measured: its budgeted number and the time
/// outside the run, from the same calls.
#[derive(Debug, Clone)]
pub struct CallMarks {
    /// `call_overhead`.
    pub overhead: ScenarioResult,
    /// `call_outside_run`.
    pub outside: ScenarioResult,
}

fn input() -> Value {
    json!({"order_id": MEASURED.0, "item_code": MEASURED.1})
}

/// One call, timed by the harness's clock and checked against the first
/// read's answer, and the lines of its stored trace.
async fn marked_read(
    scenario: &str,
    state: &Reading,
    clock: &impl Clock,
) -> Result<(Duration, Vec<TraceLine>), BenchError> {
    let input = input();
    let call = state.machine.call_tool(scenario, TOOL, &input);
    let started = clock.now();
    let (value, run) = call.await?;
    let took = clock.now().saturating_sub(started);
    if value != state.reference {
        return Err(BenchError::operation(
            scenario,
            format!("a read returned {value}, not {}", state.reference),
        ));
    }
    let lines = state.machine.trace(scenario, &run).await?;
    Ok((took, lines))
}

/// The end line's marks, held to what any true marks are: a call asks the
/// policy engine once at least and the ledger at least once, the parts fit
/// inside the run, and the run inside the call.
fn marks_of(scenario: &str, lines: &[TraceLine], took: Duration) -> Result<TimeMarks, BenchError> {
    let Some(TraceLine::End(end)) = lines.last() else {
        return Err(BenchError::operation(scenario, "the trace has no end line"));
    };
    let marks = end
        .marks
        .ok_or_else(|| BenchError::operation(scenario, "the trace's end line has no time marks"))?;
    let parts = marks
        .policy_scheduling_and_ledger_us()
        .saturating_add(marks.session.took_us)
        .saturating_add(marks.loading.took_us);
    let whole = u64::try_from(took.as_micros()).unwrap_or(u64::MAX);
    if marks.policy.calls == 0
        || marks.ledger.calls == 0
        || parts > marks.run_us
        || marks.run_us > whole
    {
        return Err(BenchError::operation(
            scenario,
            format!("the time marks do not hold together: {marks:?} in a call of {whole} us"),
        ));
    }
    Ok(marks)
}

fn micros(us: u64) -> Duration {
    Duration::from_micros(us)
}

fn result(name: &str, samples: &[Duration]) -> Result<ScenarioResult, BenchError> {
    Ok(ScenarioResult {
        name: name.to_owned(),
        operation: Stats::from_durations(samples).ok_or(BenchError::NoIterations)?,
        baseline: None,
        overhead: None,
        resident_bytes: None,
        size_bytes: None,
    })
}

fn median_us(samples: &[u64]) -> u64 {
    Stats::from_nanos(samples).map_or(0, |stats| stats.median_ns)
}

/// Reads one line item through a verified `http` binding, as
/// `http_operation` does, and takes from each run's trace the time its
/// policy check, scheduling, and ledger writes took together. The same calls
/// give the time outside the engine's run of them.
///
/// # Errors
///
/// Returns an error if the plan has no measured iteration, or if the setup,
/// a read, an export, or the marks fail.
pub async fn call_marks(plan: &Plan, clock: &impl Clock) -> Result<CallMarks, BenchError> {
    if plan.iterations == 0 {
        return Err(BenchError::NoIterations);
    }
    let state = reading(CALL_OVERHEAD, true).await?;
    for _ in 0..plan.warmup_iterations {
        marked_read(CALL_OVERHEAD, &state, clock).await?;
    }
    let mut budgeted = Vec::new();
    let mut outside = Vec::new();
    let mut parts: [Vec<u64>; 5] = Default::default();
    for _ in 0..plan.iterations {
        let (took, lines) = marked_read(CALL_OVERHEAD, &state, clock).await?;
        let marks = marks_of(CALL_OVERHEAD, &lines, took)?;
        budgeted.push(micros(marks.policy_scheduling_and_ledger_us()));
        outside.push(took.saturating_sub(micros(marks.run_us)));
        for (part, us) in parts.iter_mut().zip([
            marks.policy.took_us,
            marks.scheduling.took_us,
            marks.ledger.took_us,
            marks.session.took_us,
            marks.run_us,
        ]) {
            part.push(us);
        }
    }
    let [policy, scheduling, ledger, session, run] = &parts;
    eprintln!(
        "{CALL_OVERHEAD}: medians: policy {} us, scheduling {} us, ledger {} us, session {} us, the run {} us",
        median_us(policy),
        median_us(scheduling),
        median_us(ledger),
        median_us(session),
        median_us(run),
    );
    Ok(CallMarks {
        overhead: result(CALL_OVERHEAD, &budgeted)?,
        outside: result(CALL_OUTSIDE_RUN, &outside)?,
    })
}

/// The pass that read through the `ui` binding: the one pass of the run.
fn ui_pass<'l>(scenario: &str, lines: &'l [TraceLine]) -> Result<&'l PassLine, BenchError> {
    let passes: Vec<&PassLine> = lines
        .iter()
        .filter_map(|line| match line {
            TraceLine::Step(step) => step.begins.as_ref(),
            _ => None,
        })
        .collect();
    match passes.as_slice() {
        [pass] if pass.mode.as_str() == "ui" => Ok(pass),
        _ => Err(BenchError::operation(
            scenario,
            format!(
                "the run did not read in one pass through the `ui` binding: {:?}",
                passes
                    .iter()
                    .map(|pass| pass.mode.as_str())
                    .collect::<Vec<_>>()
            ),
        )),
    }
}

/// Reads one line item through the `ui` binding, as `ui_step` does, on a
/// browser the setup and the warm-up already started and signed in, and
/// takes from each run's trace the time preparing the driver took: the
/// session's state placed in the browser and the observing tab opened.
///
/// # Errors
///
/// Returns an error if the plan has no measured iteration, or if the setup,
/// a read, an export, or the trace fail.
pub async fn warm_tab(plan: &Plan, clock: &impl Clock) -> Result<ScenarioResult, BenchError> {
    if plan.iterations == 0 {
        return Err(BenchError::NoIterations);
    }
    let state = reading(WARM_TAB, false).await?;
    for _ in 0..plan.warmup_iterations {
        marked_read(WARM_TAB, &state, clock).await?;
    }
    let mut prepared = Vec::new();
    let mut queued = Vec::new();
    for _ in 0..plan.iterations {
        let (took, lines) = marked_read(WARM_TAB, &state, clock).await?;
        marks_of(WARM_TAB, &lines, took)?;
        let pass = ui_pass(WARM_TAB, &lines)?;
        prepared.push(micros(pass.prepare_us));
        queued.push(pass.queue_us);
    }
    eprintln!(
        "{WARM_TAB}: median wait for the session's lease before it {} us",
        median_us(&queued)
    );
    result(WARM_TAB, &prepared)
}

#[cfg(test)]
mod tests {
    use a2m_core::Digest;
    use a2m_logs::{EndLine, Mark};

    use super::*;

    fn ended(marks: Option<TimeMarks>) -> Vec<TraceLine> {
        vec![TraceLine::End(EndLine {
            records: 1,
            kept: 1,
            signature: Digest::of(b"a trace"),
            limitations: Vec::new(),
            marks,
        })]
    }

    fn mark(calls: u32, took_us: u64) -> Mark {
        Mark { calls, took_us }
    }

    #[test]
    fn marks_that_do_not_hold_together_are_never_a_number() {
        let good = TimeMarks {
            run_us: 5_000,
            policy: mark(1, 300),
            ledger: mark(5, 2_000),
            loading: mark(1, 10),
            ..TimeMarks::default()
        };
        let call = Duration::from_millis(9);
        assert_eq!(marks_of("t", &ended(Some(good)), call).unwrap(), good);
        // A trace with no marks, a call that asked no policy, parts longer
        // than the run, and a run longer than the call are each refused.
        assert!(marks_of("t", &ended(None), call).is_err());
        let no_policy = TimeMarks {
            policy: Mark::default(),
            ..good
        };
        assert!(marks_of("t", &ended(Some(no_policy)), call).is_err());
        let too_long = TimeMarks {
            ledger: mark(5, 6_000),
            ..good
        };
        assert!(marks_of("t", &ended(Some(too_long)), call).is_err());
        assert!(marks_of("t", &ended(Some(good)), Duration::from_millis(4)).is_err());
        assert!(marks_of("t", &[], call).is_err());
    }
}
