//! The budgets: one data file, `bench/budgets.json`, that mirrors the budget
//! table of spec section 225 by scenario name.
//!
//! A scenario is budgeted when this file names it. A budget whose scenario has
//! not been measured is listed as not measured, never as passing. The limits
//! are targets until measured; what fails a build is a regression, which
//! [`crate::compare::compare`] decides.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::BenchError;
use crate::harness::ScenarioResult;

/// The version of the budget file this harness reads.
pub const VERSION: u32 = 1;

/// The budget file built into the harness.
const BUILT_IN: &str = include_str!("../budgets.json");

/// The budgets, and the rule a regression is judged by.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetFile {
    /// Always [`VERSION`].
    pub version: u32,
    /// The document the table is mirrored from, relative to the repository root.
    pub source: String,
    /// A budgeted scenario that slows by more than this many percent is a regression.
    pub regression_percent: u32,
    /// A change no larger than this many microseconds is never a regression.
    pub noise_floor_us: u64,
    /// One entry per scenario that has a budget.
    pub budgets: Vec<Budget>,
}

/// The budget of one scenario.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    /// The scenario's name. A result with this name is held to this budget.
    pub scenario: String,
    /// The row of the budget table, by the text of its first cell.
    pub path: String,
    /// Which of the scenario's numbers the budget is about.
    pub metric: Metric,
    /// The most the number should be.
    pub limit: Limit,
    /// A noise floor for this scenario alone, in microseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub noise_floor_us: Option<u64>,
}

/// Which number of a scenario a budget is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Metric {
    /// The median time of the operation itself.
    Median,
    /// The median time of the operation beyond the baseline's median.
    OverheadMedian,
    /// The resident memory of what the scenario ran, in bytes.
    ResidentMemory,
    /// The size of the program's file, in bytes.
    ProgramSize,
}

impl Metric {
    /// This number in a scenario's result: nanoseconds for a time. `None` when
    /// the result does not hold it, which is the same as not measured.
    #[must_use]
    pub fn value_in(self, result: &ScenarioResult) -> Option<i64> {
        match self {
            Self::Median => Some(i64::try_from(result.operation.median_ns).unwrap_or(i64::MAX)),
            Self::OverheadMedian => result.overhead.map(|o| o.median_ns),
            Self::ResidentMemory => result
                .resident_bytes
                .map(|bytes| i64::try_from(bytes).unwrap_or(i64::MAX)),
            Self::ProgramSize => result
                .size_bytes
                .map(|bytes| i64::try_from(bytes).unwrap_or(i64::MAX)),
        }
    }

    /// The number a comparison holds against a stored one: the floor of what
    /// the run measured, not its middle (decision 0399).
    ///
    /// A time is its fastest iteration, and an overhead is the fastest
    /// operation less the fastest baseline. Load on a shared machine only ever
    /// adds time, so the fastest iteration moves far less between two runs of
    /// the same build than the median does. A memory or a size has one
    /// reading, which is its own floor.
    #[must_use]
    pub fn floor_in(self, result: &ScenarioResult) -> Option<i64> {
        let nanos = |ns: u64| i64::try_from(ns).unwrap_or(i64::MAX);
        match self {
            Self::Median => Some(nanos(result.operation.min_ns)),
            Self::OverheadMedian => result
                .baseline
                .map(|b| nanos(result.operation.min_ns).saturating_sub(nanos(b.min_ns))),
            Self::ResidentMemory | Self::ProgramSize => self.value_in(result),
        }
    }
}

/// A limit as the budget table writes it: a number and a unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limit {
    /// The number.
    pub value: u64,
    /// Its unit.
    pub unit: Unit,
}

impl Limit {
    /// The limit in the unit results are held in: nanoseconds for a time,
    /// bytes for memory.
    #[must_use]
    pub fn amount(self) -> i64 {
        let each: u64 = match self.unit {
            Unit::Ms => 1_000_000,
            Unit::S => 1_000_000_000,
            Unit::Mb => 1_000_000,
        };
        i64::try_from(self.value.saturating_mul(each)).unwrap_or(i64::MAX)
    }
}

/// The unit of a limit, spelled as the budget table spells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Unit {
    /// Milliseconds.
    #[serde(rename = "ms")]
    Ms,
    /// Seconds.
    #[serde(rename = "s")]
    S,
    /// Megabytes.
    #[serde(rename = "MB")]
    Mb,
}

impl Unit {
    /// The unit as the budget table spells it.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ms => "ms",
            Self::S => "s",
            Self::Mb => "MB",
        }
    }
}

impl BudgetFile {
    /// The budgets built into the harness: `bench/budgets.json` as it was when
    /// the harness was built.
    ///
    /// # Errors
    ///
    /// Returns an error if the built-in file is not a valid budget file. A test
    /// holds that it is.
    pub fn built_in() -> Result<Self, BenchError> {
        Self::parse(BUILT_IN, Path::new("bench/budgets.json"))
    }

    /// Reads a budget file from disk.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or is not a valid budget file.
    pub fn read(path: &Path) -> Result<Self, BenchError> {
        let text = std::fs::read_to_string(path).map_err(|source| BenchError::File {
            path: path.to_path_buf(),
            source,
        })?;
        Self::parse(&text, path)
    }

    fn parse(text: &str, path: &Path) -> Result<Self, BenchError> {
        let format = |message: String| BenchError::Format {
            path: path.to_path_buf(),
            message,
        };
        let file: Self = serde_json::from_str(text).map_err(|e| format(e.to_string()))?;
        if file.version != VERSION {
            return Err(format(format!(
                "the budget file has version {}, and this harness reads version {VERSION}",
                file.version
            )));
        }
        for (index, budget) in file.budgets.iter().enumerate() {
            if file.budgets[..index]
                .iter()
                .any(|earlier| earlier.scenario == budget.scenario)
            {
                return Err(format(format!(
                    "scenario {} has two budgets",
                    budget.scenario
                )));
            }
        }
        Ok(file)
    }

    /// The budget of the scenario with this name, if it has one.
    #[must_use]
    pub fn budget(&self, scenario: &str) -> Option<&Budget> {
        self.budgets.iter().find(|b| b.scenario == scenario)
    }

    /// The noise floor that applies to a budget, in nanoseconds.
    #[must_use]
    pub fn noise_floor_ns(&self, budget: &Budget) -> u64 {
        budget
            .noise_floor_us
            .unwrap_or(self.noise_floor_us)
            .saturating_mul(1_000)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    use super::*;

    /// The rows of the budget table of section 225: first cell, second cell.
    fn budget_table() -> Vec<(String, String)> {
        let section = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("docs")
            .join("spec")
            .join("2-kernel")
            .join("225-performance.md");
        let text = std::fs::read_to_string(section).unwrap();
        let rows: Vec<(String, String)> = text
            .lines()
            .skip_while(|line| !line.starts_with("| Path |"))
            .skip(2)
            .take_while(|line| line.starts_with('|'))
            .map(|line| {
                let cells: Vec<&str> = line.trim().trim_matches('|').split('|').collect();
                assert_eq!(cells.len(), 2, "a budget row has two cells: {line}");
                (cells[0].trim().to_owned(), cells[1].trim().to_owned())
            })
            .collect();
        assert!(!rows.is_empty(), "the budget table was not found");
        rows
    }

    #[test]
    fn the_built_in_budgets_mirror_the_table_of_section_225() {
        let file = BudgetFile::built_in().unwrap();
        assert_eq!(file.regression_percent, 10, "section 225: more than 10%");
        let table = budget_table();

        // Every budget names a row, and the row states the budget's limit.
        for budget in &file.budgets {
            let row = table
                .iter()
                .find(|(path, _)| *path == budget.path)
                .unwrap_or_else(|| panic!("no row named {:?} in section 225", budget.path));
            let limit = format!(
                "under {} {}",
                budget.limit.value,
                budget.limit.unit.as_str()
            );
            assert!(
                row.1.to_lowercase().contains(&limit.to_lowercase()),
                "the row {:?} does not say {limit:?}",
                budget.path
            );
        }
        // Every row has a budget.
        let covered: BTreeSet<&str> = file.budgets.iter().map(|b| b.path.as_str()).collect();
        let rows: BTreeSet<&str> = table.iter().map(|(path, _)| path.as_str()).collect();
        assert_eq!(covered, rows);
    }

    #[test]
    fn memory_is_read_from_the_result_and_a_time_is_not_taken_for_it() {
        use crate::harness::ScenarioResult;
        use crate::stats::Stats;
        let operation = Stats::from_nanos(&[1, 2, 3]).unwrap();
        let mut result = ScenarioResult {
            name: "idle_memory".to_owned(),
            operation,
            baseline: None,
            overhead: None,
            resident_bytes: None,
            size_bytes: None,
        };
        assert_eq!(Metric::ResidentMemory.value_in(&result), None);
        result.resident_bytes = Some(37_000_000);
        assert_eq!(Metric::ResidentMemory.value_in(&result), Some(37_000_000));
        let limit = Limit {
            value: 50,
            unit: Unit::Mb,
        };
        assert!(37_000_000 < limit.amount());
    }

    #[test]
    fn a_limit_is_held_in_nanoseconds_or_bytes() {
        let limit = |value, unit| Limit { value, unit }.amount();
        assert_eq!(limit(5, Unit::Ms), 5_000_000);
        assert_eq!(limit(1, Unit::S), 1_000_000_000);
        assert_eq!(limit(50, Unit::Mb), 50_000_000);
    }

    #[test]
    fn a_budget_file_with_a_scenario_twice_or_another_version_is_refused() {
        let file = BudgetFile::built_in().unwrap();
        let path = Path::new("planted.json");

        let mut twice = file.clone();
        twice.budgets.push(file.budgets[0].clone());
        let text = serde_json::to_string(&twice).unwrap();
        let error = BudgetFile::parse(&text, path).unwrap_err();
        assert!(error.to_string().contains("two budgets"), "{error}");

        let mut newer = file;
        newer.version = 2;
        let text = serde_json::to_string(&newer).unwrap();
        assert!(matches!(
            BudgetFile::parse(&text, path),
            Err(BenchError::Format { .. })
        ));
    }
}
