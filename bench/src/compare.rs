//! Compares a new result with a stored one and decides whether a budgeted
//! scenario has regressed.
//!
//! What is compared is the floor of each number, not its middle: the fastest
//! iteration of a time, the fastest operation less the fastest baseline of an
//! overhead ([`crate::budgets::Metric::floor_in`], decision 0399). Load on a
//! shared machine only adds time, so the floor holds still where the median
//! moves threefold between two runs of the same build. The limit of a budget
//! is still held to the median, in [`standing`].
//!
//! A budgeted scenario has regressed when its number rose by more than the
//! regression percentage of the budget file **and** by more than the noise
//! floor. The second condition is what keeps microsecond jitter from failing a
//! build: ten percent of a very small number is smaller than what two runs of
//! the same code differ by.
//!
//! A scenario that was measured in the stored result and is absent from the
//! new one also fails: a benchmark that stops running must not look like a
//! benchmark that passes. A budget that neither result measured is listed as
//! not measured. All arithmetic is in whole numbers.

use std::fmt::Write as _;

use crate::budgets::{Budget, BudgetFile};
use crate::error::BenchError;
use crate::report::{RunReport, micros};

/// What the comparison found for one budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Neither result holds the budget's number. Not a pass; nothing is known.
    NotMeasured,
    /// The new result holds it and the stored one does not: nothing to compare with yet.
    FirstMeasurement,
    /// The stored result holds it and the new one does not. Fails.
    Missing,
    /// Both hold it, and it did not regress.
    Within,
    /// Both hold it, and it rose by more than the percentage and the noise floor. Fails.
    Regression,
}

impl Verdict {
    /// Whether this verdict fails the comparison.
    #[must_use]
    pub fn fails(self) -> bool {
        matches!(self, Self::Missing | Self::Regression)
    }

    /// The verdict in words.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotMeasured => "not measured",
            Self::FirstMeasurement => "first measurement",
            Self::Missing => "MISSING",
            Self::Within => "within",
            Self::Regression => "REGRESSION",
        }
    }
}

/// One budget, the two numbers, and the verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetLine {
    /// The budget.
    pub budget: Budget,
    /// The budget's number in the stored result, if it was measured there.
    pub stored: Option<i64>,
    /// The budget's number in the new result, if it was measured there.
    pub new: Option<i64>,
    /// What that comes to.
    pub verdict: Verdict,
}

impl BudgetLine {
    /// Whether the new number is over the budget's limit. The limits are
    /// targets until measured, so this is shown and does not fail a comparison.
    #[must_use]
    pub fn over_limit(&self) -> bool {
        self.new.is_some_and(|new| new > self.budget.limit.amount())
    }
}

/// The comparison of a new result with a stored one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comparison {
    /// One line per budget, in the order of the budget file.
    pub lines: Vec<BudgetLine>,
    /// The scenarios without a budget that both results measured: name, stored
    /// median, new median, in nanoseconds. Shown, never judged.
    pub unbudgeted: Vec<(String, u64, u64)>,
}

impl Comparison {
    /// Whether any budgeted scenario regressed or went missing.
    #[must_use]
    pub fn failed(&self) -> bool {
        self.lines.iter().any(|line| line.verdict.fails())
    }

    /// The comparison as lines for a person to read, times in microseconds.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(
            out,
            "{:<22} {:>12} {:>12} {:>12}  verdict",
            "budgeted scenario", "limit us/KB", "stored", "new"
        );
        for line in &self.lines {
            let unit = line.budget.limit.unit;
            let shown = |value: Option<i64>| {
                value.map_or_else(|| "-".to_owned(), |value| show(unit, value))
            };
            let limit = show(unit, line.budget.limit.amount());
            let over = if line.over_limit() {
                ", over the target"
            } else {
                ""
            };
            let _ = writeln!(
                out,
                "{:<22} {:>12} {:>12} {:>12}  {}{over}",
                line.budget.scenario,
                limit,
                shown(line.stored),
                shown(line.new),
                line.verdict.as_str(),
            );
        }
        for (name, stored, new) in &self.unbudgeted {
            let _ = writeln!(
                out,
                "{:<22} {:>12} {:>12} {:>12}  no budget",
                name,
                "-",
                micros(i128::from(*stored)),
                micros(i128::from(*new)),
            );
        }
        out
    }
}

/// Whether a rise from `stored` to `new` is a regression: more than `percent`
/// percent, and more than the noise floor.
#[must_use]
pub fn is_regression(stored: i64, new: i64, percent: u32, noise_floor_ns: u64) -> bool {
    let (stored, new) = (i128::from(stored), i128::from(new));
    let over_the_floor = new - stored > i128::from(noise_floor_ns);
    let over_the_percentage = new * 100 > stored * (100 + i128::from(percent));
    over_the_floor && over_the_percentage
}

/// Compares a new result with a stored one, budget by budget.
///
/// `noise_floor_us`, when given, replaces every noise floor of the budget file.
///
/// # Errors
///
/// Returns an error if the two results were taken on different operating
/// systems or architectures; such numbers say nothing about each other.
pub fn compare(
    budgets: &BudgetFile,
    stored: &RunReport,
    new: &RunReport,
    noise_floor_us: Option<u64>,
) -> Result<Comparison, BenchError> {
    if (stored.os.as_str(), stored.arch.as_str()) != (new.os.as_str(), new.arch.as_str()) {
        return Err(BenchError::NotComparable(format!(
            "the stored result is from {} on {}, and the new one from {} on {}",
            stored.os, stored.arch, new.os, new.arch
        )));
    }
    let number = |budget: &Budget, report: &RunReport| {
        report
            .scenario(&budget.scenario)
            .and_then(|result| budget.metric.floor_in(result))
    };
    let lines = budgets
        .budgets
        .iter()
        .map(|budget| {
            let floor = noise_floor_us.map_or_else(
                || budgets.noise_floor_ns(budget),
                |us| us.saturating_mul(1_000),
            );
            let (was, is) = (number(budget, stored), number(budget, new));
            let verdict = match (was, is) {
                (None, None) => Verdict::NotMeasured,
                (None, Some(_)) => Verdict::FirstMeasurement,
                (Some(_), None) => Verdict::Missing,
                (Some(was), Some(is)) => {
                    if is_regression(was, is, budgets.regression_percent, floor) {
                        Verdict::Regression
                    } else {
                        Verdict::Within
                    }
                }
            };
            BudgetLine {
                budget: budget.clone(),
                stored: was,
                new: is,
                verdict,
            }
        })
        .collect();
    let unbudgeted = new
        .scenarios
        .iter()
        .filter(|result| budgets.budget(&result.name).is_none())
        .filter_map(|result| {
            let before = stored.scenario(&result.name)?;
            Some((
                result.name.clone(),
                before.operation.median_ns,
                result.operation.median_ns,
            ))
        })
        .collect();
    Ok(Comparison { lines, unbudgeted })
}

/// A number as the table shows it: a time in microseconds, memory in
/// kilobytes with its unit, so that a row never passes for the other kind.
fn show(unit: crate::budgets::Unit, value: i64) -> String {
    match unit {
        crate::budgets::Unit::Mb => format!("{} KB", micros(value)),
        _ => micros(value),
    }
}

/// The budgets against one result alone, with no stored result to compare
/// with: what a run prints so that every unmeasured budget is seen as such.
#[must_use]
pub fn standing(budgets: &BudgetFile, report: &RunReport) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{:<22} {:>12} {:>12}  standing",
        "budgeted scenario", "limit us/KB", "measured"
    );
    for budget in &budgets.budgets {
        let measured = report
            .scenario(&budget.scenario)
            .and_then(|result| budget.metric.value_in(result));
        let unit = budget.limit.unit;
        let limit = show(unit, budget.limit.amount());
        let (shown, standing) = match measured {
            None => ("-".to_owned(), "not measured"),
            Some(value) if value > budget.limit.amount() => (show(unit, value), "over the target"),
            Some(value) => (show(unit, value), "within the target"),
        };
        let _ = writeln!(
            out,
            "{:<22} {:>12} {:>12}  {standing}",
            budget.scenario, limit, shown
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{Overhead, Plan, ScenarioResult};
    use crate::stats::Stats;

    fn stats(median_ns: u64) -> Stats {
        Stats {
            count: 100,
            min_ns: median_ns,
            median_ns,
            p95_ns: median_ns,
            p99_ns: median_ns,
            max_ns: median_ns,
        }
    }

    /// A scenario whose direct request takes 400 us and whose overhead is as given.
    fn with_overhead(name: &str, overhead_ns: u64) -> ScenarioResult {
        let baseline = stats(400_000);
        let operation = stats(400_000 + overhead_ns);
        ScenarioResult {
            name: name.to_owned(),
            overhead: Some(Overhead::between(&operation, &baseline)),
            resident_bytes: None,
            size_bytes: None,
            operation,
            baseline: Some(baseline),
        }
    }

    fn report(scenarios: Vec<ScenarioResult>) -> RunReport {
        RunReport::new(None, Plan::default(), scenarios)
    }

    fn verdict_of(comparison: &Comparison, scenario: &str) -> Verdict {
        comparison
            .lines
            .iter()
            .find(|line| line.budget.scenario == scenario)
            .unwrap()
            .verdict
    }

    #[test]
    fn a_planted_regression_on_a_budgeted_scenario_fails() {
        let budgets = BudgetFile::built_in().unwrap();
        // The overhead of an `http` operation goes from 1 ms to 1.2 ms: 20% and 200 us.
        let stored = report(vec![with_overhead("http_operation", 1_000_000)]);
        let new = report(vec![with_overhead("http_operation", 1_200_000)]);
        let comparison = compare(&budgets, &stored, &new, None).unwrap();
        assert_eq!(
            verdict_of(&comparison, "http_operation"),
            Verdict::Regression
        );
        assert!(comparison.failed());
        assert!(comparison.render().contains("REGRESSION"));
    }

    #[test]
    fn jitter_under_the_noise_floor_passes_even_when_it_is_over_ten_percent() {
        let budgets = BudgetFile::built_in().unwrap();
        assert_eq!(budgets.noise_floor_us, 100);
        // From 20 us to 60 us is three times as slow, and 40 us: jitter.
        let stored = report(vec![with_overhead("http_operation", 20_000)]);
        let new = report(vec![with_overhead("http_operation", 60_000)]);
        let comparison = compare(&budgets, &stored, &new, None).unwrap();
        assert_eq!(verdict_of(&comparison, "http_operation"), Verdict::Within);
        assert!(!comparison.failed());

        // The same change fails when the caller sets the floor below it.
        let strict = compare(&budgets, &stored, &new, Some(10)).unwrap();
        assert_eq!(verdict_of(&strict, "http_operation"), Verdict::Regression);
    }

    #[test]
    fn a_rise_within_ten_percent_passes_however_large() {
        let budgets = BudgetFile::built_in().unwrap();
        // A `ui` step goes from 40 ms to 43 ms: 3 ms is far over the floor, and 7.5%.
        let stored = report(vec![with_overhead("ui_step", 40_000_000)]);
        let new = report(vec![with_overhead("ui_step", 43_000_000)]);
        let comparison = compare(&budgets, &stored, &new, None).unwrap();
        assert_eq!(verdict_of(&comparison, "ui_step"), Verdict::Within);
        assert!(!comparison.failed());
    }

    #[test]
    fn the_rule_is_strictly_more_than_the_percentage_and_the_floor() {
        // Exactly ten percent is not more than ten percent.
        assert!(!is_regression(1_000_000, 1_100_000, 10, 0));
        assert!(is_regression(1_000_000, 1_100_001, 10, 0));
        // Exactly the floor is not more than the floor.
        assert!(!is_regression(100_000, 200_000, 10, 100_000));
        assert!(is_regression(100_000, 200_001, 10, 100_000));
        // Getting faster is never a regression, and a negative overhead is only noise.
        assert!(!is_regression(1_000_000, 500_000, 10, 0));
        assert!(!is_regression(-30_000, 40_000, 10, 100_000));
        assert!(is_regression(-30_000, 400_000, 10, 100_000));
    }

    #[test]
    fn an_unmeasured_budget_is_listed_as_not_measured_and_never_as_passing() {
        let budgets = BudgetFile::built_in().unwrap();
        // The sample scenario has no budget; nothing budgeted has been measured.
        let direct = ScenarioResult {
            name: "direct_read".to_owned(),
            operation: stats(400_000),
            baseline: None,
            overhead: None,
            resident_bytes: None,
            size_bytes: None,
        };
        let stored = report(vec![direct.clone()]);
        let new = report(vec![direct]);
        let comparison = compare(&budgets, &stored, &new, None).unwrap();
        assert_eq!(comparison.lines.len(), budgets.budgets.len());
        assert!(
            comparison
                .lines
                .iter()
                .all(|line| line.verdict == Verdict::NotMeasured)
        );
        assert!(!comparison.failed());
        assert_eq!(
            comparison.unbudgeted,
            [("direct_read".to_owned(), 400_000, 400_000)]
        );
        let text = standing(&budgets, &new);
        assert_eq!(text.matches("not measured").count(), budgets.budgets.len());
        assert!(!text.contains("within"));
    }

    #[test]
    fn a_benchmark_that_stopped_running_fails_and_a_new_one_does_not() {
        let budgets = BudgetFile::built_in().unwrap();
        let with = report(vec![with_overhead("http_operation", 1_000_000)]);
        let without = report(Vec::new());

        let gone = compare(&budgets, &with, &without, None).unwrap();
        assert_eq!(verdict_of(&gone, "http_operation"), Verdict::Missing);
        assert!(gone.failed());

        let arrived = compare(&budgets, &without, &with, None).unwrap();
        assert_eq!(
            verdict_of(&arrived, "http_operation"),
            Verdict::FirstMeasurement
        );
        assert!(!arrived.failed());

        // A scenario under an overhead budget that reports no baseline has not measured it.
        let no_baseline = report(vec![ScenarioResult {
            name: "http_operation".to_owned(),
            operation: stats(1_400_000),
            baseline: None,
            overhead: None,
            resident_bytes: None,
            size_bytes: None,
        }]);
        let unmeasured = compare(&budgets, &with, &no_baseline, None).unwrap();
        assert_eq!(verdict_of(&unmeasured, "http_operation"), Verdict::Missing);
    }

    #[test]
    fn results_from_different_systems_are_not_compared_and_a_number_over_its_limit_is_shown() {
        let budgets = BudgetFile::built_in().unwrap();
        let stored = report(vec![with_overhead("http_operation", 16_000_000)]);
        let new = report(vec![with_overhead("http_operation", 16_000_000)]);
        let comparison = compare(&budgets, &stored, &new, None).unwrap();
        // 16 ms is over the 15 ms target; the target is shown and does not fail.
        assert!(comparison.lines.iter().any(BudgetLine::over_limit));
        assert!(comparison.render().contains("over the target"));
        assert!(!comparison.failed());

        let mut elsewhere = new;
        elsewhere.os = "another-system".to_owned();
        assert!(matches!(
            compare(&budgets, &stored, &elsewhere, None),
            Err(BenchError::NotComparable(_))
        ));
    }
    /// A scenario whose iterations are as given: the floor and the middle apart.
    fn with_spread(name: &str, min_ns: u64, median_ns: u64) -> ScenarioResult {
        let operation = Stats {
            min_ns,
            ..stats(median_ns)
        };
        let baseline = Stats {
            min_ns: 400_000,
            ..stats(400_000)
        };
        ScenarioResult {
            name: name.to_owned(),
            overhead: Some(Overhead::between(&operation, &baseline)),
            resident_bytes: None,
            size_bytes: None,
            operation,
            baseline: Some(baseline),
        }
    }

    #[test]
    fn a_median_that_moves_threefold_on_a_busy_machine_is_not_a_regression_while_the_floor_holds() {
        let budgets = BudgetFile::built_in().unwrap();
        // The same build twice: its fastest read is 9.4 ms both times, and its
        // median moved from 10 ms to 31 ms because the machine was busy.
        let stored = report(vec![with_spread("http_operation", 9_400_000, 10_400_000)]);
        let new = report(vec![with_spread("http_operation", 9_450_000, 31_400_000)]);
        let comparison = compare(&budgets, &stored, &new, None).unwrap();
        assert_eq!(verdict_of(&comparison, "http_operation"), Verdict::Within);
        assert!(!comparison.failed());
    }

    #[test]
    fn a_floor_that_rises_fails_even_when_the_median_does_not_move() {
        let budgets = BudgetFile::built_in().unwrap();
        // The fastest read goes from 9 ms to 11 ms: no run was quiet enough to
        // see the old time, so the code is slower, whatever the median says.
        let stored = report(vec![with_spread("http_operation", 9_000_000, 12_000_000)]);
        let new = report(vec![with_spread("http_operation", 11_000_000, 12_000_000)]);
        let comparison = compare(&budgets, &stored, &new, None).unwrap();
        assert_eq!(
            verdict_of(&comparison, "http_operation"),
            Verdict::Regression
        );
    }

    #[test]
    fn a_program_that_grows_by_more_than_ten_percent_fails() {
        let budgets = BudgetFile::built_in().unwrap();
        let sized = |bytes: u64| ScenarioResult {
            name: "program_size".to_owned(),
            operation: stats(0),
            baseline: None,
            overhead: None,
            resident_bytes: None,
            size_bytes: Some(bytes),
        };
        let stored = report(vec![sized(20_000_000)]);
        let grown = compare(&budgets, &stored, &report(vec![sized(23_000_000)]), None).unwrap();
        assert_eq!(verdict_of(&grown, "program_size"), Verdict::Regression);
        let same = compare(&budgets, &stored, &report(vec![sized(21_000_000)]), None).unwrap();
        assert_eq!(verdict_of(&same, "program_size"), Verdict::Within);
        // A size over the 25 MB target is shown, as any number over its limit is.
        let over = compare(
            &budgets,
            &report(vec![sized(26_000_000)]),
            &report(vec![sized(26_000_000)]),
            None,
        )
        .unwrap();
        assert!(
            over.lines
                .iter()
                .any(|l| l.budget.scenario == "program_size" && l.over_limit())
        );
    }
}
