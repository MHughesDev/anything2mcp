//! The two commands of the `a2m-bench` program: `run` and `compare`.
//!
//! The program is a tool of the build, not a face of the product, so its
//! commands are not actions of the product's registry.

use std::path::PathBuf;

use crate::budgets::BudgetFile;
use crate::compare::{compare, standing};
use crate::error::BenchError;
use crate::harness::{MonotonicClock, Plan};
use crate::report::RunReport;
use crate::scenarios;

/// How the program is used.
pub const USAGE: &str = "\
Usage:
  a2m-bench run --out <folder> [--iterations <n>] [--warmup <n>] [--scenario <name>]...
  a2m-bench compare --baseline <file> --new <file> [--budgets <file>] [--noise-floor-us <n>]

run      Runs the scenarios against the test application and writes one JSON
         result file into the folder. The commit, when the environment variable
         A2M_BENCH_COMMIT gives one, is stored in the file and in its name.
compare  Compares a new result with a stored one. Exits with 1 when a budgeted
         scenario slowed by more than the regression percentage and the noise
         floor, or was measured before and is missing now.

Exit status: 0 done, 1 a regression was found, 2 the command could not be carried out.
";

/// One parsed command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Run scenarios and write the result file.
    Run {
        /// The folder the result file is written into.
        out: PathBuf,
        /// How many times each scenario is run.
        plan: Plan,
        /// The scenarios to run; every scenario when empty.
        scenarios: Vec<String>,
    },
    /// Compare a new result with a stored one.
    Compare {
        /// The stored result.
        baseline: PathBuf,
        /// The new result.
        new: PathBuf,
        /// A budget file to use in place of the built-in one.
        budgets: Option<PathBuf>,
        /// A noise floor, in microseconds, in place of the budget file's.
        noise_floor_us: Option<u64>,
    },
    /// Show how the program is used.
    Help,
}

/// What a command printed and whether it found a regression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// The text for standard output.
    pub text: String,
    /// True when a comparison failed. The program then exits with 1.
    pub regression: bool,
}

impl Command {
    /// Reads a command from the program's arguments, without the program's name.
    ///
    /// # Errors
    ///
    /// Returns a usage error for an unknown command or option, a missing or
    /// repeated value, or a number that is not one.
    pub fn parse<I>(arguments: I) -> Result<Self, BenchError>
    where
        I: IntoIterator<Item = String>,
    {
        let mut arguments = arguments.into_iter();
        let usage = |message: String| BenchError::Usage(message);
        match arguments.next().as_deref() {
            None | Some("help" | "--help" | "-h") => Ok(Self::Help),
            Some("run") => {
                let mut out = None;
                let mut plan = Plan::default();
                let mut scenarios = Vec::new();
                while let Some(option) = arguments.next() {
                    let value = value_of(&option, arguments.next())?;
                    match option.as_str() {
                        "--out" => set_once(&mut out, PathBuf::from(value), &option)?,
                        "--iterations" => plan.iterations = number(&option, &value)?,
                        "--warmup" => plan.warmup_iterations = number(&option, &value)?,
                        "--scenario" => scenarios.push(value),
                        _ => return Err(usage(format!("run has no option {option}"))),
                    }
                }
                if plan.iterations == 0 {
                    return Err(BenchError::NoIterations);
                }
                let out = out.ok_or_else(|| usage("run needs --out <folder>".to_owned()))?;
                Ok(Self::Run {
                    out,
                    plan,
                    scenarios,
                })
            }
            Some("compare") => {
                let (mut baseline, mut new, mut budgets) = (None, None, None);
                let mut noise_floor_us = None;
                while let Some(option) = arguments.next() {
                    let value = value_of(&option, arguments.next())?;
                    match option.as_str() {
                        "--baseline" => set_once(&mut baseline, PathBuf::from(value), &option)?,
                        "--new" => set_once(&mut new, PathBuf::from(value), &option)?,
                        "--budgets" => set_once(&mut budgets, PathBuf::from(value), &option)?,
                        "--noise-floor-us" => {
                            set_once(&mut noise_floor_us, number(&option, &value)?, &option)?;
                        }
                        _ => return Err(usage(format!("compare has no option {option}"))),
                    }
                }
                Ok(Self::Compare {
                    baseline: baseline
                        .ok_or_else(|| usage("compare needs --baseline <file>".to_owned()))?,
                    new: new.ok_or_else(|| usage("compare needs --new <file>".to_owned()))?,
                    budgets,
                    noise_floor_us,
                })
            }
            Some(other) => Err(usage(format!("there is no command {other}"))),
        }
    }

    /// Carries the command out. `commit` is the commit a run measures, if known.
    ///
    /// # Errors
    ///
    /// Returns an error if a scenario fails or a file cannot be read or
    /// written. A regression is not an error: it is in the outcome.
    pub async fn execute(self, commit: Option<String>) -> Result<Outcome, BenchError> {
        match self {
            Self::Help => Ok(Outcome {
                text: USAGE.to_owned(),
                regression: false,
            }),
            Self::Run {
                out,
                plan,
                scenarios,
            } => {
                let budgets = BudgetFile::built_in()?;
                let clock = MonotonicClock::start();
                let results = scenarios::run_named(&scenarios, &plan, &clock).await?;
                let report = RunReport::new(commit, plan, results);
                let path = report.write_to(&out)?;
                let text = format!(
                    "{}\n{}\nwrote {}\n",
                    report.table(),
                    standing(&budgets, &report),
                    path.display()
                );
                Ok(Outcome {
                    text,
                    regression: false,
                })
            }
            Self::Compare {
                baseline,
                new,
                budgets,
                noise_floor_us,
            } => {
                let budgets = match budgets {
                    Some(path) => BudgetFile::read(&path)?,
                    None => BudgetFile::built_in()?,
                };
                let stored = RunReport::read(&baseline)?;
                let new = RunReport::read(&new)?;
                let comparison = compare(&budgets, &stored, &new, noise_floor_us)?;
                let regression = comparison.failed();
                let summary = if regression {
                    "A budgeted scenario regressed or is missing."
                } else {
                    "No budgeted scenario regressed."
                };
                Ok(Outcome {
                    text: format!("{}\n{summary}\n", comparison.render()),
                    regression,
                })
            }
        }
    }
}

fn value_of(option: &str, value: Option<String>) -> Result<String, BenchError> {
    value.ok_or_else(|| BenchError::Usage(format!("{option} needs a value")))
}

fn set_once<T>(slot: &mut Option<T>, value: T, option: &str) -> Result<(), BenchError> {
    if slot.replace(value).is_some() {
        return Err(BenchError::Usage(format!("{option} was given twice")));
    }
    Ok(())
}

fn number<T: std::str::FromStr>(option: &str, value: &str) -> Result<T, BenchError> {
    value
        .parse()
        .map_err(|_| BenchError::Usage(format!("{option} needs a whole number, not {value}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(line: &str) -> Result<Command, BenchError> {
        Command::parse(line.split_whitespace().map(str::to_owned))
    }

    #[test]
    fn the_two_commands_are_read_from_their_arguments() {
        assert_eq!(
            parse("run --out results --iterations 20 --warmup 3 --scenario direct_read").unwrap(),
            Command::Run {
                out: PathBuf::from("results"),
                plan: Plan {
                    warmup_iterations: 3,
                    iterations: 20,
                },
                scenarios: vec!["direct_read".to_owned()],
            }
        );
        assert_eq!(
            parse("run --out results").unwrap(),
            Command::Run {
                out: PathBuf::from("results"),
                plan: Plan::default(),
                scenarios: Vec::new(),
            }
        );
        assert_eq!(
            parse("compare --new b.json --baseline a.json --noise-floor-us 250").unwrap(),
            Command::Compare {
                baseline: PathBuf::from("a.json"),
                new: PathBuf::from("b.json"),
                budgets: None,
                noise_floor_us: Some(250),
            }
        );
        assert_eq!(parse("").unwrap(), Command::Help);
    }

    #[test]
    fn a_wrong_command_line_is_a_usage_error() {
        for line in [
            "measure",
            "run",
            "run --out",
            "run --out a --out b",
            "run --out a --iterations many",
            "run --out a --fast yes",
            "compare --baseline a.json",
            "compare --baseline a.json --new b.json --noise-floor-us -1",
        ] {
            assert!(
                matches!(parse(line), Err(BenchError::Usage(_))),
                "{line:?} should be a usage error"
            );
        }
        assert!(matches!(
            parse("run --out a --iterations 0"),
            Err(BenchError::NoIterations)
        ));
    }
}
