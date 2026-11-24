//! Runs a scenario: warm-up, then measured iterations, timed with a monotonic clock.
//!
//! The harness measures overhead beyond the test stand's own time. A scenario
//! that supplies a baseline operation, which is the same request made directly
//! to the stand, is reported three ways: its own timings, the baseline's, and
//! the difference between the two.

use std::future::Future;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::error::BenchError;
use crate::stats::{Stats, signed_difference};

/// A monotonic clock: the time since some fixed moment, never going back.
///
/// The harness takes time only through this, so a test can fix it.
pub trait Clock {
    /// The time since the clock's fixed moment.
    fn now(&self) -> Duration;
}

/// The monotonic clock of the operating system.
#[derive(Debug, Clone, Copy)]
pub struct MonotonicClock {
    origin: Instant,
}

impl MonotonicClock {
    /// A clock whose fixed moment is now.
    #[must_use]
    pub fn start() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Clock for MonotonicClock {
    fn now(&self) -> Duration {
        self.origin.elapsed()
    }
}

/// How many times each scenario is run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    /// Iterations run first and not measured, so that connections and caches are warm.
    pub warmup_iterations: u32,
    /// Iterations that are timed. At least one.
    pub iterations: u32,
}

impl Default for Plan {
    fn default() -> Self {
        Self {
            warmup_iterations: 50,
            iterations: 500,
        }
    }
}

/// One thing to measure: a name, a setup, and an operation.
///
/// The futures are awaited on the task that runs the harness, one at a time, so
/// they need not be `Send`.
pub trait Scenario {
    /// What the setup makes and every iteration uses, such as a running test
    /// stand and a warm client. It is dropped when the scenario has been run.
    type State;

    /// The scenario's name, as the budget file and the results spell it.
    fn name(&self) -> &str;

    /// Makes everything the operation needs. Run once, and not timed.
    fn setup(&self) -> impl Future<Output = Result<Self::State, BenchError>>;

    /// The operation that is measured. An error stops the run.
    fn operation(&self, state: &Self::State) -> impl Future<Output = Result<(), BenchError>>;

    /// The same request made directly to the test stand, or `None` for a
    /// scenario with nothing to be measured against.
    ///
    /// A scenario answers the same way every time: always a baseline or never.
    fn baseline(
        &self,
        state: &Self::State,
    ) -> Option<impl Future<Output = Result<(), BenchError>>> {
        let _ = state;
        None::<std::future::Ready<Result<(), BenchError>>>
    }
}

/// The operation's time beyond the baseline's, in nanoseconds.
///
/// Each number is the operation's statistic less the baseline's same statistic.
/// It is signed: when the two are close, noise can put the baseline ahead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Overhead {
    /// At the median.
    pub median_ns: i64,
    /// At the 95th percentile.
    pub p95_ns: i64,
    /// At the 99th percentile.
    pub p99_ns: i64,
}

impl Overhead {
    /// The operation's statistics less the baseline's.
    #[must_use]
    pub fn between(operation: &Stats, baseline: &Stats) -> Self {
        Self {
            median_ns: signed_difference(operation.median_ns, baseline.median_ns),
            p95_ns: signed_difference(operation.p95_ns, baseline.p95_ns),
            p99_ns: signed_difference(operation.p99_ns, baseline.p99_ns),
        }
    }
}

/// What one scenario measured.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScenarioResult {
    /// The scenario's name.
    pub name: String,
    /// The timings of the operation.
    pub operation: Stats,
    /// The timings of the baseline operation, when the scenario has one.
    pub baseline: Option<Stats>,
    /// The difference, when the scenario has a baseline.
    pub overhead: Option<Overhead>,
    /// The resident memory, in bytes, of what the scenario ran, by the median
    /// of its readings, when the scenario reads it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resident_bytes: Option<u64>,
}

/// Runs one scenario: its setup, the warm-up, then the measured iterations.
///
/// With a baseline, every iteration runs both operations, and which goes first
/// alternates, so that neither is favored by a drift in the machine's load or
/// by always following the other.
///
/// # Errors
///
/// Returns an error if the plan has no measured iteration, or if the setup, an
/// operation, or a baseline operation fails. Nothing is reported for a scenario
/// that failed part of the way.
pub async fn run_scenario<S: Scenario>(
    scenario: &S,
    plan: &Plan,
    clock: &impl Clock,
) -> Result<ScenarioResult, BenchError> {
    if plan.iterations == 0 {
        return Err(BenchError::NoIterations);
    }
    let state = scenario.setup().await?;
    let has_baseline = scenario.baseline(&state).is_some();

    for iteration in 0..plan.warmup_iterations {
        one_iteration(scenario, &state, clock, has_baseline, iteration).await?;
    }
    let mut operation = Vec::new();
    let mut baseline = Vec::new();
    for iteration in 0..plan.iterations {
        let (took, baseline_took) =
            one_iteration(scenario, &state, clock, has_baseline, iteration).await?;
        operation.push(took);
        baseline.extend(baseline_took);
    }

    let operation = Stats::from_durations(&operation).ok_or(BenchError::NoIterations)?;
    let baseline = Stats::from_durations(&baseline);
    Ok(ScenarioResult {
        name: scenario.name().to_owned(),
        overhead: baseline.as_ref().map(|b| Overhead::between(&operation, b)),
        operation,
        baseline,
        resident_bytes: None,
    })
}

/// Runs the operation once, and the baseline once if there is one, timing each.
async fn one_iteration<S: Scenario>(
    scenario: &S,
    state: &S::State,
    clock: &impl Clock,
    has_baseline: bool,
    iteration: u32,
) -> Result<(Duration, Option<Duration>), BenchError> {
    if !has_baseline {
        return Ok((time_operation(scenario, state, clock).await?, None));
    }
    if iteration.is_multiple_of(2) {
        let baseline = time_baseline(scenario, state, clock).await?;
        let operation = time_operation(scenario, state, clock).await?;
        Ok((operation, baseline))
    } else {
        let operation = time_operation(scenario, state, clock).await?;
        let baseline = time_baseline(scenario, state, clock).await?;
        Ok((operation, baseline))
    }
}

async fn time_operation<S: Scenario>(
    scenario: &S,
    state: &S::State,
    clock: &impl Clock,
) -> Result<Duration, BenchError> {
    // The future is made before the clock is read, as the baseline's is.
    let operation = scenario.operation(state);
    let started = clock.now();
    operation.await?;
    Ok(clock.now().saturating_sub(started))
}

async fn time_baseline<S: Scenario>(
    scenario: &S,
    state: &S::State,
    clock: &impl Clock,
) -> Result<Option<Duration>, BenchError> {
    let Some(baseline) = scenario.baseline(state) else {
        return Ok(None);
    };
    let started = clock.now();
    baseline.await?;
    Ok(Some(clock.now().saturating_sub(started)))
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    /// A clock that moves only when a scenario says time has passed.
    #[derive(Default)]
    struct FixedClock(Cell<Duration>);

    impl FixedClock {
        fn pass(&self, micros: u64) {
            self.0.set(self.0.get() + Duration::from_micros(micros));
        }
    }

    impl Clock for FixedClock {
        fn now(&self) -> Duration {
            self.0.get()
        }
    }

    /// A scenario whose operations take scripted times on the fixed clock.
    struct Scripted<'a> {
        clock: &'a FixedClock,
        with_baseline: bool,
        fail_at: Option<u32>,
    }

    #[derive(Default)]
    struct Counters {
        operations: Cell<u32>,
        baselines: Cell<u32>,
    }

    impl Scenario for Scripted<'_> {
        type State = Counters;

        fn name(&self) -> &str {
            "scripted"
        }

        async fn setup(&self) -> Result<Counters, BenchError> {
            // Time spent in setup must never show in a measurement.
            self.clock.pass(1_000_000);
            Ok(Counters::default())
        }

        async fn operation(&self, state: &Counters) -> Result<(), BenchError> {
            let n = state.operations.get();
            state.operations.set(n + 1);
            if self.fail_at == Some(n) {
                return Err(BenchError::operation("scripted", "the stand answered 503"));
            }
            // The operation costs the baseline's time plus 2 ms, plus 1 us per call so far.
            self.clock.pass(5_000 + 2_000 + u64::from(n));
            Ok(())
        }

        fn baseline(
            &self,
            state: &Counters,
        ) -> Option<impl Future<Output = Result<(), BenchError>>> {
            self.with_baseline.then_some(async move {
                let n = state.baselines.get();
                state.baselines.set(n + 1);
                self.clock.pass(5_000 + u64::from(n));
                Ok(())
            })
        }
    }

    fn run<S: Scenario>(
        scenario: &S,
        plan: &Plan,
        clock: &FixedClock,
    ) -> Result<ScenarioResult, BenchError> {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(run_scenario(scenario, plan, clock))
    }

    #[test]
    fn the_report_gives_the_operation_the_baseline_and_the_difference() {
        let clock = FixedClock::default();
        let scenario = Scripted {
            clock: &clock,
            with_baseline: true,
            fail_at: None,
        };
        let plan = Plan {
            warmup_iterations: 10,
            iterations: 100,
        };
        let result = run(&scenario, &plan, &clock).unwrap();

        // Calls 0 to 9 were the warm-up; the measured calls are 10 to 109.
        let us = |n: u64| n * 1_000;
        assert_eq!(result.name, "scripted");
        assert_eq!(
            result.operation,
            Stats {
                count: 100,
                min_ns: us(7_010),
                median_ns: us(7_059),
                p95_ns: us(7_104),
                p99_ns: us(7_108),
                max_ns: us(7_109),
            }
        );
        assert_eq!(
            result.baseline,
            Some(Stats {
                count: 100,
                min_ns: us(5_010),
                median_ns: us(5_059),
                p95_ns: us(5_104),
                p99_ns: us(5_108),
                max_ns: us(5_109),
            })
        );
        // The overhead is the 2 ms the operation adds, at every percentile.
        assert_eq!(
            result.overhead,
            Some(Overhead {
                median_ns: 2_000_000,
                p95_ns: 2_000_000,
                p99_ns: 2_000_000,
            })
        );
    }

    #[test]
    fn a_scenario_without_a_baseline_reports_no_overhead() {
        let clock = FixedClock::default();
        let scenario = Scripted {
            clock: &clock,
            with_baseline: false,
            fail_at: None,
        };
        let plan = Plan {
            warmup_iterations: 0,
            iterations: 3,
        };
        let result = run(&scenario, &plan, &clock).unwrap();
        assert_eq!(result.operation.count, 3);
        assert_eq!(result.operation.min_ns, 7_000_000);
        assert_eq!(result.baseline, None);
        assert_eq!(result.overhead, None);
    }

    #[test]
    fn a_failed_operation_stops_the_run_and_nothing_is_reported() {
        let clock = FixedClock::default();
        let scenario = Scripted {
            clock: &clock,
            with_baseline: true,
            fail_at: Some(4),
        };
        let plan = Plan {
            warmup_iterations: 2,
            iterations: 10,
        };
        let error = run(&scenario, &plan, &clock).unwrap_err();
        assert!(
            matches!(error, BenchError::Operation { ref scenario, .. } if scenario == "scripted")
        );

        let no_iterations = Plan {
            warmup_iterations: 5,
            iterations: 0,
        };
        assert!(matches!(
            run(&scenario, &no_iterations, &clock),
            Err(BenchError::NoIterations)
        ));
    }
}
