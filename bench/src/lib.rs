//! The benchmark harness: measures the product's overhead beyond a test stand's own time.
//!
//! A [`harness::Scenario`] is a name, a setup, and an operation. The harness
//! runs warm-up and measured iterations, times each with a monotonic clock, and
//! reduces the timings to a count, the minimum, the median, the 95th and 99th
//! percentiles, and the maximum ([`stats::Stats`]). A scenario can supply a
//! baseline operation, the same request made directly to the test stand; the
//! result then holds both sets of numbers and the difference, which is the
//! overhead the budgets of spec section 225 are about.
//!
//! One run is written as one JSON file ([`report::RunReport`]). The budgets are
//! one data file, `bench/budgets.json` ([`budgets::BudgetFile`]). A comparison
//! of a new result with a stored one ([`compare::compare`]) fails when a
//! budgeted scenario regressed by more than ten percent and the noise floor.
//!
//! The harness is measurement equipment. It sits outside the crate layering,
//! as the test stands do, and nothing in the product depends on it. A new
//! benchmark is a new [`harness::Scenario`] in [`scenarios`], added with the
//! code it measures.

pub mod budgets;
pub mod cli;
pub mod compare;
pub mod error;
pub mod harness;
pub mod measure;
pub mod promotion;
pub mod report;
pub mod scenarios;
pub mod stats;

pub use budgets::BudgetFile;
pub use compare::{Comparison, Verdict};
pub use error::BenchError;
pub use harness::{Clock, MonotonicClock, Overhead, Plan, Scenario, ScenarioResult, run_scenario};
pub use report::RunReport;
pub use stats::Stats;
