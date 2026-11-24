//! The scenarios the harness can run today, by name.
//!
//! A scenario of code that does not exist yet is not here. Its budget is in
//! `bench/budgets.json`, and it is reported as not measured until it is.

use example_app::harness::TestApp;

use crate::error::BenchError;
use crate::harness::{Clock, Plan, Scenario, ScenarioResult, run_scenario};
use crate::measure::{
    COLD_BROWSER_START, HTTP_OPERATION, HttpOperation, IDLE_MEMORY, LIST_TOOLS, ListTools,
    MOST_BROWSER_ITERATIONS, PROGRAM_SIZE, START_TO_READY, START_TO_READY_VAULT_CLOSED, UI_STEP,
    UiStep, cold_browser_start, idle_memory, program_size, start_to_ready,
    start_to_ready_vault_closed,
};
use crate::promotion::{MOST_ITERATIONS, PROMOTION_SPEEDUP, PromotionSpeedup};

/// The name of the direct read scenario.
pub const DIRECT_READ: &str = "direct_read";

/// The name of every scenario, in the order they are run.
pub const NAMES: &[&str] = &[
    DIRECT_READ,
    LIST_TOOLS,
    HTTP_OPERATION,
    UI_STEP,
    COLD_BROWSER_START,
    START_TO_READY,
    START_TO_READY_VAULT_CLOSED,
    IDLE_MEMORY,
    PROGRAM_SIZE,
    PROMOTION_SPEEDUP,
];

/// The path of the one record a single read asks for: a seeded line item of
/// the test application.
pub const SEEDED_ITEM_PATH: &str = "/api/orders/A-100/items/L-101";

/// A running test application and a warm client, for a direct read.
///
/// This is the baseline of every later benchmark: a scenario that reads through
/// the product keeps one of these in its state and gives [`Stand::read_directly`]
/// as its baseline operation, so both go to the same application.
#[derive(Debug)]
pub struct Stand {
    app: TestApp,
    item_url: String,
}

impl Stand {
    /// Starts a test application with the seed data on a free local port.
    ///
    /// # Errors
    ///
    /// Returns an error, naming the scenario, if the application could not be started.
    pub async fn start(scenario: &str) -> Result<Self, BenchError> {
        let app = TestApp::start()
            .await
            .map_err(|e| BenchError::setup(scenario, e))?;
        let item_url = app.url(SEEDED_ITEM_PATH);
        Ok(Self { app, item_url })
    }

    /// The running application, for a scenario that points the product at it.
    #[must_use]
    pub fn app(&self) -> &TestApp {
        &self.app
    }

    /// Reads the seeded line item over HTTP, straight from the application:
    /// one request, the whole answer read, and nothing else.
    ///
    /// # Errors
    ///
    /// Returns an error, naming the scenario, if the request failed or the
    /// answer was not a success with a body.
    pub async fn read_directly(&self, scenario: &str) -> Result<(), BenchError> {
        let failed = |e: reqwest::Error| BenchError::operation(scenario, e);
        let response = self
            .app
            .http()
            .get(&self.item_url)
            .send()
            .await
            .map_err(failed)?;
        let status = response.status();
        let body = response.bytes().await.map_err(failed)?;
        if !status.is_success() || body.is_empty() {
            return Err(BenchError::operation(
                scenario,
                format!(
                    "the test application answered {status} with {} bytes",
                    body.len()
                ),
            ));
        }
        Ok(())
    }
}

/// A direct read of one record from the test application over HTTP.
///
/// It measures the test stand's own time, as a client on the same machine sees
/// it. It has no baseline, because it is the baseline, and no budget, because
/// that time is not the product's.
#[derive(Debug, Clone, Copy, Default)]
pub struct DirectRead;

impl Scenario for DirectRead {
    type State = Stand;

    fn name(&self) -> &str {
        DIRECT_READ
    }

    async fn setup(&self) -> Result<Stand, BenchError> {
        Stand::start(DIRECT_READ).await
    }

    async fn operation(&self, stand: &Stand) -> Result<(), BenchError> {
        stand.read_directly(DIRECT_READ).await
    }
}

/// Runs the named scenarios, or every scenario when no name is given.
///
/// # Errors
///
/// Returns an error if a name is not a scenario's, before anything is run, or
/// if a scenario fails.
pub async fn run_named(
    names: &[String],
    plan: &Plan,
    clock: &impl Clock,
) -> Result<Vec<ScenarioResult>, BenchError> {
    if let Some(unknown) = names.iter().find(|name| !NAMES.contains(&name.as_str())) {
        return Err(BenchError::UnknownScenario(unknown.clone()));
    }
    let mut results = Vec::new();
    for name in NAMES {
        if !names.is_empty() && !names.iter().any(|wanted| wanted == name) {
            continue;
        }
        // One arm per scenario: each has its own state type.
        let result = match *name {
            DIRECT_READ => run_scenario(&DirectRead, plan, clock).await?,
            PROMOTION_SPEEDUP => {
                // A read through a browser is slow: a few dozen make a median.
                let plan = Plan {
                    warmup_iterations: plan.warmup_iterations.min(3),
                    iterations: plan.iterations.min(MOST_ITERATIONS),
                };
                run_scenario(&PromotionSpeedup, &plan, clock).await?
            }
            LIST_TOOLS => run_scenario(&ListTools, plan, clock).await?,
            HTTP_OPERATION => run_scenario(&HttpOperation, plan, clock).await?,
            UI_STEP => run_scenario(&UiStep, &browser_plan(plan), clock).await?,
            COLD_BROWSER_START => cold_browser_start(&browser_plan(plan), clock).await?,
            START_TO_READY | START_TO_READY_VAULT_CLOSED | IDLE_MEMORY | PROGRAM_SIZE => {
                let result = match *name {
                    PROGRAM_SIZE => program_size()?,
                    START_TO_READY => start_to_ready(plan, clock).await?,
                    START_TO_READY_VAULT_CLOSED => start_to_ready_vault_closed(plan, clock).await?,
                    _ => idle_memory(plan, clock).await?,
                };
                match result {
                    Some(result) => result,
                    None => {
                        eprintln!(
                            "{name}: not measured; the program `anything2mcp` is not beside the harness (build it, or set A2M_BENCH_PROGRAM), the harness is not a release build (the size is of a release program), or the system gives no reading"
                        );
                        continue;
                    }
                }
            }
            other => return Err(BenchError::UnknownScenario(other.to_owned())),
        };
        results.push(result);
    }
    Ok(results)
}

/// A scenario that reads through a browser runs a few dozen times: that is
/// what a median needs.
fn browser_plan(plan: &Plan) -> Plan {
    Plan {
        warmup_iterations: plan.warmup_iterations.min(3),
        iterations: plan.iterations.min(MOST_BROWSER_ITERATIONS),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::MonotonicClock;

    #[tokio::test]
    async fn the_direct_read_runs_against_the_test_application() {
        let plan = Plan {
            warmup_iterations: 2,
            iterations: 5,
        };
        let results = run_named(&[DIRECT_READ.to_owned()], &plan, &MonotonicClock::start())
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        let read = &results[0];
        assert_eq!(read.name, "direct_read");
        assert_eq!(read.operation.count, 5);
        assert!(read.operation.min_ns > 0, "a request takes some time");
        assert!(read.operation.min_ns <= read.operation.median_ns);
        assert!(read.operation.median_ns <= read.operation.p95_ns);
        assert!(read.operation.p95_ns <= read.operation.p99_ns);
        assert!(read.operation.p99_ns <= read.operation.max_ns);
        assert_eq!((read.baseline, read.overhead), (None, None));
    }

    #[tokio::test]
    async fn the_direct_read_asks_for_a_record_the_application_holds() {
        // Ground truth from the control interface: the seeded record is there.
        let stand = Stand::start("test").await.unwrap();
        assert!(
            stand
                .app()
                .stored_item("A-100", "L-101")
                .await
                .unwrap()
                .is_some()
        );
        stand.read_directly("test").await.unwrap();

        // A read that the application refuses is an error, never a timing.
        let app = TestApp::start().await.unwrap();
        let missing = Stand {
            item_url: app.url("/api/orders/A-100/items/no-such-item"),
            app,
        };
        let error = missing.read_directly("test").await.unwrap_err();
        assert!(matches!(error, BenchError::Operation { .. }), "{error}");
    }

    #[tokio::test]
    async fn an_unknown_scenario_is_refused_before_anything_runs() {
        let names = ["direct_read".to_owned(), "no_such_scenario".to_owned()];
        let error = run_named(&names, &Plan::default(), &MonotonicClock::start())
            .await
            .unwrap_err();
        assert!(matches!(error, BenchError::UnknownScenario(name) if name == "no_such_scenario"));
    }
}
